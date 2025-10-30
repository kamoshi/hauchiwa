#![cfg(not(doctest))]

mod assets;
#[cfg(feature = "asyncrt")]
mod asyncrt;
mod content;
#[cfg(feature = "images")]
mod images;
pub mod glob;
mod script;
#[cfg(feature = "styles")]
pub mod styles;
mod svelte;

use crate::{
    error::{BuildError},
    Hash32,
};
use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

pub use assets::glob_assets;
#[cfg(feature = "asyncrt")]
pub use asyncrt::async_asset;
pub use content::{glob_content, json, yaml, Content};
#[cfg(feature = "images")]
pub use images::{glob_images, Image};
pub use script::{build_scripts, Script, Scripts};
#[cfg(feature = "styles")]
pub use styles::{build_styles, Style, Styles};
pub use svelte::{build_svelte, glob_svelte, Svelte};

/// Build execution context, providing facilities for storing artifacts in a
/// content-addressed cache and output directory.
///
/// `Runtime` abstracts filesystem interactions related to build artifact
pub struct File<T> {
    pub path: Utf8PathBuf,
    pub metadata: T,
}

/// storage, enabling immutability and reproducibility guarantees through
/// content hashing.
#[derive(Clone)]
pub struct Runtime;

impl Runtime {
    /// Persist the given binary `data` under a hash-based path with the
    /// specified file extension `ext`.
    ///
    /// This method computes a 32-bit hash of `data` to uniquely identify the
    /// artifact. It stores the artifact in a local cache directory. The
    /// returned path is a stable, canonicalized URI rooted at `/hash/`.
    ///
    /// # Parameters
    /// - `data`: The raw bytes of the artifact to store.
    /// - `ext`: The file extension (e.g., "js", "css", "webp") used for the
    ///   output artifact, influencing MIME-type recognition and loader behavior.
    ///
    /// # Returns
    /// - On success, returns the logical asset path as a `Utf8PathBuf` rooted
    ///   under `/hash/`, suitable for inclusion in HTML.
    /// - On failure, returns a `BuildError` for I/O or hashing errors.
    pub fn store(&self, data: &[u8], ext: &str) -> Result<Utf8PathBuf, BuildError> {
        let hash = Hash32::hash(data);
        let hash = hash.to_hex();

        let path_temp = Utf8Path::new(".cache/hash").join(&hash);
        let path_dist = Utf8Path::new("dist/hash").join(&hash).with_extension(ext);
        let path_root = Utf8Path::new("/hash/").join(&hash).with_extension(ext);

        if !path_temp.exists() {
            fs::create_dir_all(".cache/hash")?;
            fs::write(&path_temp, data)?;
        }

        let dir = path_dist.parent().unwrap_or(&path_dist);
        fs::create_dir_all(dir)?;
        fs::copy(&path_temp, &path_dist)?;

        Ok(path_root)
    }
}
