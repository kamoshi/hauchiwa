#![cfg(not(doctest))]

mod assets;
#[cfg(feature = "asyncrt")]
mod asyncrt;
mod content;
#[cfg(feature = "images")]
mod images;
mod script;
#[cfg(feature = "styles")]
mod styles;
mod svelte;

use std::{
    collections::{HashMap, HashSet},
    fs,
    sync::Arc,
};

use camino::{Utf8Path, Utf8PathBuf};
use glob::Pattern;
use petgraph::graph::NodeIndex;

use crate::{
    error::{BuildError, LoaderError},
    gitmap::GitRepo,
    task::Dynamic,
    Hash32, Item, Task,
};

pub use assets::glob_assets;
#[cfg(feature = "asyncrt")]
pub use asyncrt::async_asset;
pub use content::{Content, glob_content, json, yaml};
#[cfg(feature = "images")]
pub use images::{Image, glob_images};
pub use script::{Script, glob_scripts};
#[cfg(feature = "styles")]
pub use styles::{Style, glob_styles};
pub use svelte::{Svelte, glob_svelte};

/// Build execution context, providing facilities for storing artifacts in a
/// content-addressed cache and output directory.
///
/// `Runtime` abstracts filesystem interactions related to build artifact
pub struct File<T> {
    pub path: Utf8PathBuf,
    pub metadata: T,
}

pub struct FileLoaderTask<R>
where
    R: Send + Sync + 'static,
{
    path_base: &'static str,
    path_glob: &'static str,
    pattern: Pattern,
    callback: Box<dyn Fn(File<Vec<u8>>) -> anyhow::Result<R> + Send + Sync>,
    is_dirty: bool,
}

impl<R> FileLoaderTask<R>
where
    R: Send + Sync + 'static,
{
    pub fn new<F>(
        path_base: &'static str,
        path_glob: &'static str,
        callback: F,
    ) -> Self
    where
        F: Fn(File<Vec<u8>>) -> anyhow::Result<R> + Send + Sync + 'static,
    {
        let pattern = Utf8Path::new(path_base).join(path_glob);
        let pattern = Pattern::new(pattern.as_str()).unwrap();

        Self {
            path_base,
            path_glob,
            pattern,
            callback: Box::new(callback),
            is_dirty: true,
        }
    }
}

impl<R> Task for FileLoaderTask<R>
where
    R: Clone + Send + Sync + 'static,
{
    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn execute(&self, _dependencies: &[Dynamic]) -> Dynamic {
        let mut results = Vec::new();

        let pattern = Utf8Path::new(self.path_base).join(self.path_glob);
        for path in glob::glob(pattern.as_str()).expect("Failed to read glob pattern") {
            match path {
                Ok(path) => {
                    let path = Utf8PathBuf::try_from(path).expect("Invalid UTF-8 path");
                    let data = fs::read(&path).expect("Unable to read file");
                    let file = File {
                        path,
                        metadata: data,
                    };
                    let result = (self.callback)(file).expect("File processing failed");
                    results.push(result);
                }
                Err(e) => eprintln!("Error processing path: {}", e),
            }
        }

        Arc::new(results)
    }

    fn on_file_change(&mut self, path: &Utf8Path) -> bool {
        if self.pattern.matches_path(path.as_std_path()) {
            self.is_dirty = true;
            true
        } else {
            false
        }
    }
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
