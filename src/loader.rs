#![cfg(not(doctest))]

mod assets;
#[cfg(feature = "asyncrt")]
mod asyncrt;
mod content;
#[cfg(feature = "images")]
mod images;
mod script;
#[cfg(feature = "styles")]
pub mod styles;
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
    error::{BuildError},
    task::Dynamic,
    Hash32, Task,
};

pub use assets::glob_assets;
#[cfg(feature = "asyncrt")]
pub use asyncrt::async_asset;
pub use content::{Content, glob_content, json, yaml};
#[cfg(feature = "images")]
pub use images::{Image, glob_images};
pub use script::{build_script, glob_scripts, Script};
#[cfg(feature = "styles")]
pub use styles::{build_style, glob_styles, Style};
pub use svelte::{build_svelte, glob_svelte, Svelte};

/// Build execution context, providing facilities for storing artifacts in a
/// content-addressed cache and output directory.
///
/// `Runtime` abstracts filesystem interactions related to build artifact
pub struct File<T> {
    pub path: Utf8PathBuf,
    pub metadata: T,
}

use crate::Globals;

pub struct FileLoaderTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    path_base: &'static str,
    path_glob: &'static str,
    pattern: Pattern,
    callback: Box<dyn Fn(&Globals<G>, File<Vec<u8>>) -> anyhow::Result<R> + Send + Sync>,
    is_dirty: bool,
    _phantom: std::marker::PhantomData<G>,
}

impl<G, R> FileLoaderTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    pub fn new<F>(
        path_base: &'static str,
        path_glob: &'static str,
        callback: F,
    ) -> Self
    where
        F: Fn(&Globals<G>, File<Vec<u8>>) -> anyhow::Result<R> + Send + Sync + 'static,
    {
        let pattern = Utf8Path::new(path_base).join(path_glob);
        let pattern = Pattern::new(pattern.as_str()).unwrap();

        Self {
            path_base,
            path_glob,
            pattern,
            callback: Box::new(callback),
            is_dirty: true,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<G, R> Task<G> for FileLoaderTask<G, R>
where
    G: Send + Sync + 'static,
    R: Clone + Send + Sync + 'static,
{
    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn execute(&self, globals: &Globals<G>, _dependencies: &[Dynamic]) -> Dynamic {
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
                    let result = (self.callback)(globals, file).expect("File processing failed");
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

pub struct BundleLoaderTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    entry_point: Utf8PathBuf,
    watch_glob: &'static str,
    pattern: Pattern,
    callback: Box<dyn Fn(&Globals<G>, File<Vec<u8>>) -> anyhow::Result<R> + Send + Sync>,
    is_dirty: bool,
    _phantom: std::marker::PhantomData<G>,
}

impl<G, R> BundleLoaderTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    pub fn new<F>(
        entry_point: &'static str,
        watch_glob: &'static str,
        callback: F,
    ) -> Self
    where
        F: Fn(&Globals<G>, File<Vec<u8>>) -> anyhow::Result<R> + Send + Sync + 'static,
    {
        let pattern = Pattern::new(watch_glob).unwrap();

        Self {
            entry_point: Utf8PathBuf::from(entry_point),
            watch_glob,
            pattern,
            callback: Box::new(callback),
            is_dirty: true,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<G, R> Task<G> for BundleLoaderTask<G, R>
where
    G: Send + Sync + 'static,
    R: Clone + Send + Sync + 'static,
{
    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn execute(&self, globals: &Globals<G>, _dependencies: &[Dynamic]) -> Dynamic {
        let path = &self.entry_point;
        let data = fs::read(path).expect("Unable to read file");
        let file = File {
            path: path.clone(),
            metadata: data,
        };
        let result = (self.callback)(globals, file).expect("File processing failed");
        Arc::new(result)
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
