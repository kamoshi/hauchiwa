//! Utilities for working with output data and paths.
//!
//! This module contains the [`Output`] struct, which represents a final output file,
//! and helper functions for path normalization and slugification.

use std::fs;
use std::io;
use std::path::Path;

use camino::Utf8Component;
use camino::{Utf8Path, Utf8PathBuf};

use crate::Many;
use crate::One;
use crate::core::Dynamic;
use crate::engine::Handle;
use crate::engine::Map;
use crate::engine::TrackerPtr;

/// Helper function to compute the bundle scope path.
///
/// It returns the "folder" that owns this piece of content.
/// - content/foo/index.md -> content/foo
/// - content/foo/bar.md -> content/foo/bar
pub fn source_to_bundle(path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
    let path = path.as_ref().with_extension("");

    // Check if the last component of the path is exactly "index.*"
    if let Some("index") = path.file_name() {
        // If it is, return its parent directory.
        // - "foo/index.html" -> parent is "foo"
        // - "/index.html"    -> parent is "/"
        // - "index.html"     -> parent is "" (empty path)
        if let Some(parent) = path.parent() {
            return parent.to_path_buf();
        }
    }

    // Otherwise, or if there's no parent (which is rare if file_name() matched),
    // return the original path converted to a Utf8PathBuf.
    path.to_path_buf()
}

/// Helper function to compute the web-accessible URL path (href).
///
/// It strips the offset and creates a pretty URL (ending in `/`).
pub fn source_to_href(path: &Utf8Path, offset: Option<&str>) -> String {
    let path = if let Some(offset) = offset {
        path.strip_prefix(offset).unwrap_or(path)
    } else {
        path
    };

    let mut url = String::from("/");

    // If it's not index.md, we need to append the stem (e.g., 'some-file')
    // If it IS index.md, we only want the parent directory structure.
    if let Some(parent) = path.parent() {
        url.push_str(parent.as_str());
    }

    let stem = path.file_stem().unwrap_or_default();
    if stem != "index" {
        if !url.ends_with('/') {
            url.push('/');
        }
        url.push_str(stem);
    }

    // Ensure trailing slash for directory-style routing
    if !url.ends_with('/') {
        url.push('/');
    }

    // Handling edge case: double slash at start if parent was empty
    if url.starts_with("//") {
        url.replace("//", "/")
    } else {
        url
    }
}

/// Helper function to compute the dist path from the href.
///
/// It appends `index.html` to the href (relative to dist root).
pub fn href_to_dist(href: &str, dist_root: impl AsRef<Utf8Path>) -> Utf8PathBuf {
    // Remove leading slash to join correctly with dist_dir
    dist_root
        .as_ref()
        .join(href.trim_start_matches('/'))
        .join("index.html")
}

/// Normalize a path, removing things like `.` and `..`.
///
/// CAUTION: This does not resolve symlinks (unlike [`std::fs::canonicalize`]).
/// This may cause incorrect or surprising behavior at times. This should be
/// used carefully. Unfortunately, [`std::fs::canonicalize`] can be hard to use
/// correctly, since it can often fail, or on Windows returns annoying device
/// paths.
///
/// Adapted from
/// <https://github.com/rust-lang/cargo/blob/f7acf448fc127df9a77c52cc2bba027790ac4931/crates/cargo-util/src/paths.rs#L76-L116>
pub(crate) fn normalize_path(path: &Utf8Path) -> Utf8PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ Utf8Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        Utf8PathBuf::from(c.as_str())
    } else {
        Utf8PathBuf::new()
    };

    for component in components {
        match component {
            Utf8Component::Prefix(..) => unreachable!(),
            Utf8Component::RootDir => {
                ret.push(Utf8Component::RootDir);
            }
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                if ret.ends_with(Utf8Component::ParentDir) {
                    ret.push(Utf8Component::ParentDir);
                } else {
                    let popped = ret.pop();
                    if !popped && !ret.has_root() {
                        ret.push(Utf8Component::ParentDir);
                    }
                }
            }
            Utf8Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}

fn normalize_path_html(path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
    let mut buffer = path.as_ref().to_path_buf();

    if let Some(file_name) = buffer.file_name() {
        if file_name == "index" || file_name.starts_with("index.") {
            buffer.set_extension("html");
        } else {
            buffer.set_extension("");
            buffer.push("index.html");
        }
    } else {
        buffer.push("index.html");
    }

    buffer
}

/// The content of an [`Output`] file.
#[derive(Debug, Clone, Hash)]
pub enum OutputData {
    /// Text content (UTF-8).
    Utf8(String),
    /// Binary content (raw bytes).
    Binary(Vec<u8>),
}

impl AsRef<[u8]> for OutputData {
    fn as_ref(&self) -> &[u8] {
        match self {
            OutputData::Utf8(s) => s.as_bytes(),
            OutputData::Binary(b) => b.as_slice(),
        }
    }
}

/// Represents a single output file to be written to the `dist` directory.
///
/// A `Output` is a common output type for tasks that generate HTML, TXT, or
/// other static assets. The build system collects all `Output` instances and
/// writes them to the filesystem.
#[derive(Debug, Clone, Hash)]
pub struct Output {
    /// The destination path of the file, relative to the `dist` directory.
    pub path: Utf8PathBuf,
    /// The content of the file to be written.
    pub data: OutputData,
}

impl Output {
    /// Starts a builder to create an Output from a source path.
    pub fn mapper(source: impl Into<Utf8PathBuf>) -> OutputBuilder {
        OutputBuilder {
            current: source.into(),
        }
    }

    /// Creates a new output with a normalized URL, suitable for HTML files.
    ///
    /// The path is automatically adjusted to create "pretty URLs". For example:
    /// - `foo/bar.html` becomes `foo/bar/index.html`
    /// - `foo/index.html` remains `foo/index.html`
    pub fn html(path: impl AsRef<Utf8Path>, data: impl Into<String>) -> Self {
        Self {
            path: normalize_path_html(path),
            data: OutputData::Utf8(data.into()),
        }
    }

    /// Creates a new output with a raw, unmodified path.
    ///
    /// This constructor is suitable for binary assets where the output path
    /// should not be altered, and the file content is provided as raw bytes.
    pub fn binary(path: impl Into<Utf8PathBuf>, data: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            data: OutputData::Binary(data.into()),
        }
    }
}

/// A helper builder to transform source paths into destination URLs.
pub struct OutputBuilder {
    current: Utf8PathBuf,
}

impl OutputBuilder {
    /// Removes a prefix from the path (e.g., "content/").
    pub fn strip_prefix(
        mut self,
        prefix: impl AsRef<Utf8Path>,
    ) -> Result<Self, crate::error::HauchiwaError> {
        self.current = self
            .current
            .strip_prefix(prefix.as_ref())
            .map(|p| p.to_path_buf())
            .map_err(|_| {
                anyhow::anyhow!(
                    "Path {} does not start with prefix {}",
                    self.current,
                    prefix.as_ref().as_str()
                )
            })
            .map_err(|e| crate::error::HauchiwaError::Build(crate::error::BuildError::Other(e)))?;
        Ok(self)
    }

    /// Applies "Pretty URL" formatting (slugification).
    /// `posts/hello.md` -> `posts/hello/index.html`
    pub fn html(mut self) -> Self {
        self.current = source_to_bundle(&self.current)
            .join("index")
            .with_extension("html");
        self
    }

    /// Sets the file extension explicitly.
    pub fn ext(mut self, extension: &str) -> Self {
        self.current.set_extension(extension);
        self
    }

    /// Finalizes the path and attaches content to produce the Output.
    pub fn content(self, body: impl Into<String>) -> Output {
        // If it's HTML, we ensure it ends in index.html for the server
        let path = if (self.current.extension() == Some("html"))
            || self.current.file_name() == Some("index")
        {
            normalize_path_html(&self.current)
        } else {
            // For non-html assets, normalize just cleans . and ..
            normalize_path(&self.current)
        };

        Output {
            path,
            data: OutputData::Utf8(body.into()),
        }
    }
}

/// A trait for handles that can be flattened into a list of Output references.
pub trait OutputHandle: Handle {
    /// Extracts a list of `Output` references from the handle's resolved value.
    fn resolve_refs(item: &Dynamic) -> (Option<TrackerPtr>, Vec<&Output>);
}

impl OutputHandle for One<Output> {
    fn resolve_refs(item: &Dynamic) -> (Option<TrackerPtr>, Vec<&Output>) {
        match item.downcast_ref::<Output>() {
            Some(item) => (None, vec![item]),
            None => unreachable!(),
        }
    }
}

impl OutputHandle for One<Vec<Output>> {
    fn resolve_refs(item: &Dynamic) -> (Option<TrackerPtr>, Vec<&Output>) {
        match item.downcast_ref::<Vec<Output>>() {
            Some(item) => (None, item.iter().collect()),
            None => unreachable!(),
        }
    }
}

impl OutputHandle for Many<Output> {
    fn resolve_refs(item: &Dynamic) -> (Option<TrackerPtr>, Vec<&Output>) {
        match item.downcast_ref::<Map<Output>>() {
            Some(map) => {
                let ptr = TrackerPtr::default();
                let mut items = Vec::new();

                {
                    let mut tracker = ptr.ptr.lock().unwrap();

                    for (key, (output, provenance)) in &map.map {
                        tracker.accessed.insert(key.clone(), *provenance);
                        items.push(output);
                    }
                }

                (Some(ptr), items)
            }
            None => unreachable!(),
        }
    }
}

/// Saves all pages to the "dist" directory.
pub(crate) fn save_pages_to_dist(pages: &[Output]) -> io::Result<()> {
    let output_dir = Path::new("dist");

    fs::create_dir_all(output_dir)?;

    for page in pages {
        let file_path = output_dir.join(&page.path);

        if let Some(parent_dir) = file_path.parent() {
            fs::create_dir_all(parent_dir)?;
        }

        fs::write(&file_path, &page.data)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_to_href() {
        // Simple file
        assert_eq!(
            source_to_href(Utf8Path::new("content/posts/hello.md"), Some("content")),
            "/posts/hello/"
        );

        // Index file
        assert_eq!(
            source_to_href(Utf8Path::new("content/posts/index.md"), Some("content")),
            "/posts/"
        );

        // No offset
        assert_eq!(
            source_to_href(Utf8Path::new("posts/hello.md"), None),
            "/posts/hello/"
        );

        // Root index
        assert_eq!(source_to_href(Utf8Path::new("index.md"), None), "/");

        // Double slash edge case (e.g. parent is empty after offset strip, but it's not index)
        assert_eq!(
            source_to_href(Utf8Path::new("content/hello.md"), Some("content")),
            "/hello/"
        );

        // Double slash edge case with index
        assert_eq!(
            source_to_href(Utf8Path::new("content/index.md"), Some("content")),
            "/"
        );

        // Deeply nested
        assert_eq!(
            source_to_href(Utf8Path::new("content/a/b/c.md"), Some("content")),
            "/a/b/c/"
        );
    }

    #[test]
    fn test_source_to_bundle() {
        // Standard file
        assert_eq!(
            source_to_bundle("content/foo/bar.md"),
            Utf8Path::new("content/foo/bar")
        );

        // Index file
        assert_eq!(
            source_to_bundle("content/foo/index.md"),
            Utf8Path::new("content/foo")
        );

        // Root index
        assert_eq!(source_to_bundle("index.md"), Utf8Path::new(""));
    }

    #[test]
    fn test_href_to_dist() {
        // Standard href
        assert_eq!(
            href_to_dist("/posts/hello/", "dist"),
            Utf8Path::new("dist/posts/hello/index.html")
        );

        // Root href
        assert_eq!(href_to_dist("/", "dist"), Utf8Path::new("dist/index.html"));

        // Nested href
        assert_eq!(
            href_to_dist("/a/b/c/", "dist"),
            Utf8Path::new("dist/a/b/c/index.html")
        );
    }
}
