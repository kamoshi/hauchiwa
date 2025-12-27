//! Utilities for working with pages and paths.
//!
//! This module contains the [`Page`] struct, which represents a final output file,
//! and helper functions for path normalization and slugification.

use camino::Utf8Component;
use camino::{Utf8Path, Utf8PathBuf};

/// index component from path
pub fn to_slug(path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
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

/// Normalize a path, removing things like `.` and `..`.
///
/// CAUTION: This does not resolve symlinks (unlike
/// [`std::fs::canonicalize`]). This may cause incorrect or surprising
/// behavior at times. This should be used carefully. Unfortunately,
/// [`std::fs::canonicalize`] can be hard to use correctly, since it can often
/// fail, or on Windows returns annoying device paths.
///
/// Adapted from
/// https://github.com/rust-lang/cargo/blob/f7acf448fc127df9a77c52cc2bba027790ac4931/crates/cargo-util/src/paths.rs#L76-L116
pub fn normalize_path(path: &Utf8Path) -> Utf8PathBuf {
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

pub fn normalize_prefixed(prefix: &str, path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
    let path = path.as_ref().strip_prefix(prefix).unwrap_or(path.as_ref());

    normalize(path)
}

pub fn normalize(path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
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

pub fn absolutize(prefix: &str, path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
    let path = path.as_ref().strip_prefix(prefix).unwrap_or(path.as_ref());
    let path = Utf8Path::new("/").join(path);

    if let Some(file_name) = path.file_name() {
        if file_name == "index" || file_name.starts_with("index.") {
            path.parent().unwrap().to_path_buf()
        } else {
            path.with_extension("")
        }
    } else {
        path
    }
}

/// Represents a single output file to be written to the `dist` directory.
///
/// A `Page` is a common output type for tasks that generate HTML, CSS, or other static assets.
/// The build system collects all `Page` instances and writes them to the filesystem.
#[derive(Debug, Clone)]
pub struct Page {
    /// The destination path of the file, relative to the `dist` directory.
    pub url: Utf8PathBuf,
    /// The content of the file to be written.
    pub content: String,
}

impl Page {
    /// Creates a new `Page` with a normalized URL, suitable for HTML files.
    ///
    /// The path is automatically adjusted to create "pretty URLs". For example:
    /// - `foo/bar.html` becomes `foo/bar/index.html`
    /// - `foo/index.html` remains `foo/index.html`
    pub fn html(path: impl AsRef<Utf8Path>, content: impl Into<String>) -> Self {
        Self {
            url: normalize(path),
            content: content.into(),
        }
    }

    /// Creates a new `Page` with a raw, unmodified path.
    ///
    /// This constructor is suitable for assets like CSS, JavaScript, or images
    /// where the output path should not be altered.
    pub fn file(path: impl Into<Utf8PathBuf>, content: impl Into<String>) -> Self {
        Self {
            url: path.into(),
            content: content.into(),
        }
    }
}

use std::fs;
use std::io;
use std::path::Path;

/// Saves all pages to the "dist" directory.
pub(crate) fn save_pages_to_dist(pages: &[Page]) -> io::Result<()> {
    let output_dir = Path::new("dist");

    fs::create_dir_all(output_dir)?;

    for page in pages {
        let file_path = output_dir.join(&page.url);

        if let Some(parent_dir) = file_path.parent() {
            fs::create_dir_all(parent_dir)?;
        }

        fs::write(&file_path, &page.content)?;
    }

    Ok(())
}
