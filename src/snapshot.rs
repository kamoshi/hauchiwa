use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use camino::{Utf8Path, Utf8PathBuf};
use petgraph::graph::NodeIndex;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::Hash32;
use crate::output::Output;

/// What a snapshot entry represents in `dist`.
///
/// The path (the `Snapshot` HashMap key) is always dist-relative
/// (e.g. `posts/hello/index.html`, `hash/abc123.png`, `styles/main.css`).
pub(crate) enum SnapshotEntry {
    /// An [`Output`] file whose content is held in memory and written by `commit()`.
    Page {
        node: NodeIndex,
        task: String,
        output: Output,
        /// Blake3 hash of `output.data`, computed once at insert time.
        /// Used by `commit_diff` to skip unchanged pages without touching the disk.
        content_hash: Hash32,
    },
    /// A content-addressed asset already written to `dist/hash/` by `Store::save()`
    /// during task execution. Tracked here so reconciliation can detect orphans.
    HashAsset {
        node: NodeIndex,
        task: String,
    },
    /// A static file copied from the source tree via `Blueprint::copy_static()`.
    /// The copy happens in `clone_static()`; this entry records provenance for
    /// the reconciliation pass.
    StaticFile {
        source: Utf8PathBuf,
    },
}

/// A virtual representation of the `dist` directory after a build.
///
/// The `Snapshot` is assembled from all task outputs before anything is
/// written to disk. It serves as the single authority on what files should
/// exist in `dist` and which task produced each one.
///
/// Conflict detection happens at insert time: if two tasks claim the same
/// output path, a warning is emitted and the first writer wins.
pub(crate) struct Snapshot {
    entries: HashMap<Utf8PathBuf, SnapshotEntry>,
}

impl Snapshot {
    pub(crate) fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Inserts an [`Output`] page. Warns if two tasks claim the same dist path.
    pub(crate) fn insert_page(&mut self, node: NodeIndex, task_name: &str, output: Output) {
        let path = output.path.clone();
        if let Some(existing) = self.entries.get(&path) {
            let existing_task = match existing {
                SnapshotEntry::Page { task, .. } | SnapshotEntry::HashAsset { task, .. } => {
                    task.as_str()
                }
                SnapshotEntry::StaticFile { .. } => "<static>",
            };
            tracing::warn!(
                "Output conflict at `{}`: produced by `{}` and `{}`",
                path,
                existing_task,
                task_name
            );
        } else {
            tracing::debug!("snapshot: page `{}` <- task `{}`", path, task_name);
            let content_hash = Hash32::hash(&output.data);
            self.entries.insert(
                path,
                SnapshotEntry::Page {
                    node,
                    task: task_name.to_string(),
                    content_hash,
                    output,
                },
            );
        }
    }

    /// Inserts a content-addressed hash asset (dist-relative path, e.g. `hash/abc.png`).
    ///
    /// Multiple tasks may reference the same content-addressed path - that is valid
    /// (same hash = same content). The first claimant is recorded for provenance.
    pub(crate) fn insert_hash_asset(
        &mut self,
        node: NodeIndex,
        task_name: &str,
        path: Utf8PathBuf,
    ) {
        self.entries
            .entry(path)
            .or_insert(SnapshotEntry::HashAsset {
                node,
                task: task_name.to_string(),
            });
    }

    /// Inserts a static file entry (dist-relative path → source path).
    ///
    /// The file is already copied to dist by `clone_static()`; this records
    /// provenance so the reconciliation pass can run without `clear_dist()`.
    pub(crate) fn insert_static_file(&mut self, dist_rel: Utf8PathBuf, source: Utf8PathBuf) {
        self.entries
            .entry(dist_rel)
            .or_insert(SnapshotEntry::StaticFile { source });
    }

    pub(crate) fn find(&self, path: &Utf8Path) -> Option<(NodeIndex, &str)> {
        match self.entries.get(path)? {
            SnapshotEntry::Page { node, task, .. } => Some((*node, task.as_str())),
            SnapshotEntry::HashAsset { node, task } => Some((*node, task.as_str())),
            SnapshotEntry::StaticFile { .. } => None,
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&Utf8PathBuf, &SnapshotEntry)> {
        self.entries.iter()
    }

    /// Total number of tracked dist files across all entry types.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Number of [`SnapshotEntry::Page`] entries (HTML/binary outputs from tasks).
    pub(crate) fn page_count(&self) -> usize {
        self.entries
            .values()
            .filter(|e| matches!(e, SnapshotEntry::Page { .. }))
            .count()
    }

    /// Full reconcile against `dist` - intended for the initial build.
    ///
    /// 1. Walks `dist` and deletes any file not present in this snapshot.
    /// 2. Writes every [`SnapshotEntry::Page`] whose content differs from
    ///    what is already on disk (blake3 comparison).
    ///
    /// [`SnapshotEntry::HashAsset`] and [`SnapshotEntry::StaticFile`] entries
    /// are already on disk before `commit()` is called.
    pub(crate) fn commit(&self) -> io::Result<()> {
        let dist = Path::new("dist");
        fs::create_dir_all(dist)?;

        tracing::debug!(
            "commit: {} total entries ({} pages, {} hash assets, {} static files)",
            self.entries.len(),
            self.entries.values().filter(|e| matches!(e, SnapshotEntry::Page { .. })).count(),
            self.entries.values().filter(|e| matches!(e, SnapshotEntry::HashAsset { .. })).count(),
            self.entries.values().filter(|e| matches!(e, SnapshotEntry::StaticFile { .. })).count(),
        );

        let desired: HashSet<Utf8PathBuf> = self.entries.keys().cloned().collect();
        let removed = remove_stale(dist, Utf8Path::new(""), &desired)?;
        if removed > 0 {
            tracing::info!("removed {} stale file(s) from dist", removed);
        }

        write_pages(dist, self.entries.iter().filter_map(|(path, entry)| match entry {
            SnapshotEntry::Page { output, content_hash, .. } => Some((path, output, content_hash)),
            _ => None,
        }))
    }

    /// Incremental diff against a previous snapshot - intended for watch rebuilds.
    ///
    /// Compared to `commit()`, this avoids walking `dist` on disk: the diff is
    /// computed entirely from the two in-memory snapshots.
    ///
    /// 1. Deletes files present in `prev` but absent from `self`.
    /// 2. Writes pages that are new or whose content hash changed.
    ///    Pages with identical hashes are skipped entirely - no disk read needed.
    pub(crate) fn commit_diff(&self, prev: &Snapshot) -> io::Result<()> {
        let dist = Path::new("dist");
        fs::create_dir_all(dist)?;

        tracing::debug!(
            "commit_diff: {} prev entries -> {} new entries",
            prev.entries.len(),
            self.entries.len(),
        );

        // Delete files that disappeared from the snapshot.
        let mut removed = 0;
        let mut dirs_to_prune: HashSet<std::path::PathBuf> = HashSet::new();
        for path in prev.entries.keys() {
            if !self.entries.contains_key(path) {
                let abs = dist.join(path.as_std_path());
                tracing::debug!("removing stale dist file: {}", path);
                fs::remove_file(&abs)?;
                removed += 1;
                if let Some(parent) = abs.parent() {
                    dirs_to_prune.insert(parent.to_path_buf());
                }
            }
        }
        if removed > 0 {
            tracing::info!("removed {} stale file(s) from dist", removed);
            prune_empty_dirs(dist, dirs_to_prune)?;
        }

        // Write pages that are new or whose content changed.
        write_pages(dist, self.entries.iter().filter_map(|(path, entry)| {
            let SnapshotEntry::Page { output, content_hash, .. } = entry else {
                return None;
            };
            match prev.entries.get(path) {
                None => {
                    tracing::debug!("new page: {}", path);
                    Some((path, output, content_hash))
                }
                Some(SnapshotEntry::Page { content_hash: prev_hash, .. }) => {
                    if prev_hash != content_hash {
                        tracing::debug!("changed page: {}", path);
                        Some((path, output, content_hash))
                    } else {
                        tracing::debug!("unchanged page, skipping: {}", path);
                        None
                    }
                }
                _ => {
                    tracing::debug!("new page (replaced non-page entry): {}", path);
                    Some((path, output, content_hash))
                }
            }
        }))
    }

    /// Converts this snapshot into a slim, serializable form suitable for
    /// persisting to disk. Page output data is not included - only the
    /// content hash is retained for future diffing.
    pub(crate) fn to_meta(&self) -> SnapshotMeta {
        let entries = self
            .entries
            .iter()
            .map(|(path, entry)| {
                let meta_entry = match entry {
                    SnapshotEntry::Page { content_hash, .. } => {
                        MetaEntry::Page { content_hash: content_hash.to_bytes() }
                    }
                    SnapshotEntry::HashAsset { .. } => MetaEntry::HashAsset,
                    SnapshotEntry::StaticFile { .. } => MetaEntry::StaticFile,
                };
                (path.to_string(), meta_entry)
            })
            .collect();
        SnapshotMeta { entries }
    }

    /// Incremental diff against a persisted snapshot - intended for cold-start
    /// builds where no in-memory previous snapshot is available.
    ///
    /// Semantics are identical to [`commit_diff`](Self::commit_diff).
    pub(crate) fn commit_diff_meta(&self, prev: &SnapshotMeta) -> io::Result<()> {
        let dist = Path::new("dist");
        fs::create_dir_all(dist)?;

        tracing::debug!(
            "commit_diff_meta: {} prev entries -> {} new entries",
            prev.entries.len(),
            self.entries.len(),
        );

        // Delete files that disappeared from the snapshot.
        let mut removed = 0;
        let mut dirs_to_prune: HashSet<std::path::PathBuf> = HashSet::new();
        for path in prev.entries.keys() {
            if !self.entries.contains_key(Utf8Path::new(path)) {
                let abs = dist.join(path.as_str());
                tracing::debug!("removing stale dist file: {}", path);
                fs::remove_file(&abs)?;
                removed += 1;
                if let Some(parent) = abs.parent() {
                    dirs_to_prune.insert(parent.to_path_buf());
                }
            }
        }
        if removed > 0 {
            tracing::info!("removed {} stale file(s) from dist", removed);
            prune_empty_dirs(dist, dirs_to_prune)?;
        }

        // Write pages that are new or whose content hash changed.
        write_pages(dist, self.entries.iter().filter_map(|(path, entry)| {
            let SnapshotEntry::Page { output, content_hash, .. } = entry else {
                return None;
            };
            match prev.entries.get(path.as_str()) {
                None => {
                    tracing::debug!("new page: {}", path);
                    Some((path, output, content_hash))
                }
                Some(MetaEntry::Page { content_hash: prev_hash }) => {
                    if prev_hash != &content_hash.to_bytes() {
                        tracing::debug!("changed page: {}", path);
                        Some((path, output, content_hash))
                    } else {
                        tracing::debug!("unchanged page, skipping: {}", path);
                        None
                    }
                }
                _ => {
                    tracing::debug!("new page (replaced non-page entry): {}", path);
                    Some((path, output, content_hash))
                }
            }
        }))
    }
}

/// Slim, serializable representation of a [`Snapshot`].
///
/// Stored at `.cache/snapshot/metadata.cbor` after each successful build.
/// Loaded on the next cold start to drive [`Snapshot::commit_diff_meta`],
/// skipping unchanged pages without a full `dist` walk.
#[derive(Serialize, Deserialize)]
pub(crate) struct SnapshotMeta {
    entries: HashMap<String, MetaEntry>,
}

#[derive(Serialize, Deserialize)]
enum MetaEntry {
    Page { content_hash: [u8; 32] },
    HashAsset,
    StaticFile,
}

impl SnapshotMeta {
    const PATH: &'static str = ".cache/snapshot/metadata.cbor";

    /// Loads the persisted snapshot meta from disk.
    ///
    /// Returns `None` if the file does not exist (e.g. first build).
    /// Returns an error for I/O or deserialization failures.
    pub(crate) fn load() -> io::Result<Option<Self>> {
        let file = match fs::File::open(Self::PATH) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        tracing::debug!("loading snapshot meta from {}", Self::PATH);
        ciborium::from_reader(file)
            .map(Some)
            .map_err(io::Error::other)
    }

    /// Persists this snapshot meta to `.cache/snapshot/metadata.cbor`.
    pub(crate) fn save(&self) -> io::Result<()> {
        fs::create_dir_all(".cache/snapshot")?;
        let file = fs::File::create(Self::PATH)?;
        tracing::debug!("saving snapshot meta to {}", Self::PATH);
        ciborium::into_writer(self, file).map_err(io::Error::other)
    }
}

/// Writes a set of pages to `dist` in parallel.
///
/// Pre-creates all unique parent directories before spawning rayon workers
/// so workers never race on directory creation.
fn write_pages<'a>(
    dist: &Path,
    pages: impl Iterator<Item = (&'a Utf8PathBuf, &'a Output, &'a Hash32)>,
) -> io::Result<()> {
    let pages: Vec<_> = pages.collect();

    let parent_dirs: HashSet<std::path::PathBuf> = pages
        .iter()
        .filter_map(|(path, _, _)| {
            dist.join(path.as_std_path()).parent().map(|p| p.to_path_buf())
        })
        .collect();
    for dir in parent_dirs {
        fs::create_dir_all(dir)?;
    }

    pages.par_iter().try_for_each(|(path, output, _hash)| {
        fs::write(dist.join(path.as_std_path()), &output.data)
    })
}

/// Removes empty directories left after deletions.
///
/// Sorts candidates deepest-first so a parent is only attempted after all
/// its children have been processed. `fs::remove_dir` is a no-op on
/// non-empty directories - errors are silently ignored.
fn prune_empty_dirs(
    dist: &Path,
    dirs: HashSet<std::path::PathBuf>,
) -> io::Result<()> {
    let mut dirs: Vec<_> = dirs.into_iter().collect();
    dirs.sort_by(|a, b| b.components().count().cmp(&a.components().count()));
    for dir in dirs {
        if dir == dist {
            continue;
        }
        if fs::remove_dir(&dir).is_ok() {
            tracing::debug!("pruned empty dir: {}", dir.display());
        }
    }
    Ok(())
}

/// Walks `dist/<rel>` and removes any file whose dist-relative path is not in `desired`.
/// Empty directories left behind after removal are pruned as well.
/// Returns the number of files deleted.
fn remove_stale(dist: &Path, rel: &Utf8Path, desired: &HashSet<Utf8PathBuf>) -> io::Result<usize> {
    let dir = if rel.as_str().is_empty() {
        dist.to_path_buf()
    } else {
        dist.join(rel.as_std_path())
    };

    let read_dir = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        // Nothing to sweep if dist doesn't exist yet (first build).
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };

    let mut removed = 0;

    for entry in read_dir {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 filename in dist")
        })?;

        let entry_rel = if rel.as_str().is_empty() {
            Utf8PathBuf::from(name_str)
        } else {
            rel.join(name_str)
        };

        let entry_abs = dist.join(entry_rel.as_std_path());
        if entry.file_type()?.is_dir() {
            removed += remove_stale(dist, &entry_rel, desired)?;
            if fs::read_dir(&entry_abs)?.next().is_none() {
                fs::remove_dir(&entry_abs)?;
            }
        } else if !desired.contains(entry_rel.as_path()) {
            tracing::debug!("removing stale dist file: {}", entry_rel);
            fs::remove_file(&entry_abs)?;
            removed += 1;
        }
    }

    Ok(removed)
}
