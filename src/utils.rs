use std::collections::HashSet;
use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use console::Style;
use indicatif::ProgressStyle;
use rayon::prelude::*;
use tracing::{Level, info, span};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::core::Hash32;
use crate::error::StepCopyStatic;

const ANSI_BLUE: Style = Style::new().blue();

#[allow(clippy::expect_used)] // hardcoded template literal — cannot fail
static PROGRESS_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos} {msg}")
        .expect("Error setting progress bar template")
        .progress_chars("#>-")
});

pub fn get_style_task() -> Result<ProgressStyle, indicatif::style::TemplateError> {
    ProgressStyle::default_spinner().template("{spinner:.blue} {msg}")
}

pub fn get_style_task_progress() -> Result<ProgressStyle, indicatif::style::TemplateError> {
    ProgressStyle::default_spinner().template("{spinner:.blue} {msg} {pos}/{len} ")
}

pub fn as_overhead(s: Instant) -> impl Display {
    let e = Instant::now();
    let f = format!("(+{}ms)", e.duration_since(s).as_millis());
    ANSI_BLUE.apply_to(f)
}

/// Returns `true` if the `dst` file is considered identical to `src`.
///
/// The check proceeds in increasing order of cost:
/// 1. **Metadata**: Fails early if `dst` is missing or file sizes differ.
/// 2. **Mtime**: Returns `true` immediately if modification times match.
/// 3. **Content**: Performs a full BLAKE3 hash comparison as a final fallback.
#[rustfmt::skip]
fn is_unchanged(src: &Path, dst: &Path) -> bool {
    let Ok(src_meta) = fs::metadata(src) else { return false };
    let Ok(dst_meta) = fs::metadata(dst) else { return false };

    // different file size
    if src_meta.len() != dst_meta.len() {
        return false;
    }

    let Ok(src_mod) = src_meta.modified() else { return false };
    let Ok(dst_mod) = dst_meta.modified() else { return false };

    // same mtime
    if src_mod == dst_mod {
        return true;
    }

    let Ok(src) = Hash32::hash_file(src) else { return false };
    let Ok(dst) = Hash32::hash_file(dst) else { return false };

    // same hash
    src == dst
}

struct FileEntry {
    src: PathBuf,
    dst: PathBuf,
    source_utf8: Utf8PathBuf,
    dist_rel: Utf8PathBuf,
}

/// Copies all static file trees configured via `Blueprint::copy_static` into `dist/`.
///
/// Returns the list of `(source_path, dist_relative_path)` pairs for every file
/// that was copied. These are inserted into the [`Snapshot`](crate::output::Snapshot)
/// by the caller so that step 4 can reconcile `dist` without `clear_dist()`.
pub fn clone_static(
    copied: &[(String, String)],
) -> Result<Vec<(Utf8PathBuf, Utf8PathBuf)>, StepCopyStatic> {
    if copied.is_empty() {
        return Ok(vec![]);
    }

    let span = span!(Level::INFO, "copy_static", indicatif.pb_show = true);
    span.pb_set_message("Copying static files...");
    span.pb_set_style(&PROGRESS_STYLE);
    let _enter = span.enter();

    let s = Instant::now();
    let mut files: Vec<FileEntry> = Vec::new();

    for (into, from) in copied {
        let path = std::path::Path::new(into);
        let mut depth = 0;
        let mut safe = true;

        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    depth -= 1;
                    if depth < 0 {
                        safe = false;
                        break;
                    }
                }
                std::path::Component::Normal(_) => {
                    depth += 1;
                }
                std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                    safe = false;
                    break;
                }
                std::path::Component::CurDir => {}
            }
        }

        if !safe {
            return Err(StepCopyStatic::UnsafeTarget(into.clone()));
        }

        let target = std::path::Path::new("dist").join(into);
        let dist_rel = Utf8Path::new(into);

        if fs::metadata(from).is_ok() {
            collect_files(from, &target, dist_rel, &mut files)?;
        }
    }

    span.pb_set_length(files.len() as u64);

    // Pre-create all destination directories before parallelising copies to
    // avoid races between concurrent `fs::copy` calls on the same new path.
    for dir in files
        .iter()
        .filter_map(|f| f.dst.parent())
        .collect::<HashSet<&Path>>()
    {
        fs::create_dir_all(dir)?;
    }

    // Hash-check and copy files in parallel.
    let entries: Vec<(Utf8PathBuf, Utf8PathBuf)> = files
        .par_iter()
        .map(|f| -> std::io::Result<(Utf8PathBuf, Utf8PathBuf)> {
            if !is_unchanged(&f.src, &f.dst) {
                fs::copy(&f.src, &f.dst)?;
            }
            span.pb_inc(1);
            Ok((f.source_utf8.clone(), f.dist_rel.clone()))
        })
        .collect::<std::io::Result<_>>()?;

    info!("Finished copying static files! {}", as_overhead(s));

    Ok(entries)
}

/// Recursively walks `src`, appending one [`FileEntry`] per file to `files`.
/// Directory creation is deferred to the caller.
fn collect_files(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
    dist_rel: &Utf8Path,
    files: &mut Vec<FileEntry>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_str().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "non-UTF-8 filename")
        })?;

        if entry.file_type()?.is_dir() {
            collect_files(
                entry.path(),
                dst.as_ref().join(&name),
                &dist_rel.join(name_str),
                files,
            )?;
        } else {
            let src_path = entry.path();
            let source_utf8 = Utf8PathBuf::try_from(src_path.clone())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            files.push(FileEntry {
                dst: dst.as_ref().join(&name),
                src: src_path,
                source_utf8,
                dist_rel: dist_rel.join(name_str),
            });
        }
    }

    Ok(())
}
