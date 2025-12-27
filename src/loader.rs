//! Loaders are tasks that ingest data from the filesystem or external sources.
//!
//! A "Loader" is typically a task with **zero dependencies** that reads files
//! matching a glob pattern, processes them (e.g., parsing frontmatter, resizing
//! images), and stores them in a [`Registry`].
//!
//! This module provides the core types for loaders:
//! - [`Registry`]: A map of paths to processed data.
//! - [`File`]: Represents a raw file read from disk.
//! - [`Runtime`]: A thread-safe helper for tasks to store artifacts (like images)
//!   and register import maps.
//!
//! It also contains the `GlobRegistryTask`, which is the workhorse for most loaders.

pub mod generic;
pub use generic::Content;

#[cfg(feature = "images")]
pub mod image;
#[cfg(feature = "images")]
pub use image::Image;

#[cfg(feature = "styles")]
pub mod css;
#[cfg(feature = "styles")]
pub use css::Stylesheet;

pub mod js;
pub use js::Script;

pub mod svelte;
pub use svelte::Svelte;

#[cfg(feature = "asyncrt")]
pub mod tokio;

use std::{collections::HashMap, fs};

use camino::{Utf8Path, Utf8PathBuf};
use glob::{Pattern, glob};
use gray_matter::engine::YAML;
use petgraph::graph::NodeIndex;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::{
    Context, Hash32,
    error::{BuildError, HauchiwaError},
    importmap::ImportMap,
    task::{Dynamic, TypedTask},
};

/// A collection of processed assets, indexed by their source file path.
///
/// `Registry<T>` is the standard return type for most loaders. It allows you to
/// access processed items (like posts or images) using their original file path.
///
/// # Example
///
/// ```rust,ignore
/// // Assuming `posts` is a Registry<Content<Post>>
/// for post in posts.values() {
///     println!("Title: {}", post.metadata.title);
/// }
///
/// let specific_post = posts.get("content/posts/hello.md")?;
/// ```
#[derive(Debug)]
pub struct Registry<T> {
    map: HashMap<camino::Utf8PathBuf, T>,
}

impl<T: Clone> Registry<T> {
    /// Retrieves a reference to the processed data for a given source path.
    ///
    /// # Errors
    ///
    /// Returns `HauchiwaError::AssetNotFound` if the path does not exist in the registry.
    pub fn get(&self, path: impl AsRef<Utf8Path>) -> Result<&T, HauchiwaError> {
        self.map
            .get(path.as_ref())
            .ok_or(HauchiwaError::AssetNotFound(
                path.as_ref().to_string().into(),
            ))
    }

    /// Returns an iterator over all items in the registry.
    pub fn values(&self) -> std::collections::hash_map::Values<'_, Utf8PathBuf, T> {
        self.map.values()
    }

    /// Finds all items whose source paths match the given glob pattern.
    ///
    /// # Returns
    ///
    /// A vector of `(Path, &Item)` tuples.
    pub fn glob(&self, pattern: &str) -> Result<Vec<(&Utf8PathBuf, &T)>, HauchiwaError> {
        let matcher = Pattern::new(pattern)?;

        let matches: Vec<_> = self
            .map
            .iter()
            .filter(|(path, _)| matcher.matches(path.as_str()))
            .collect();

        Ok(matches)
    }
}

/// A raw file read from the filesystem.
///
/// This struct is passed to the callback of custom loaders.
pub struct File {
    /// The path to the source file.
    pub path: Utf8PathBuf,
    /// The raw binary content of the file.
    pub data: Box<[u8]>,
}

/// A helper for managing side effects and imports within a task.
///
/// `Runtime` is passed to task callbacks to allow them to:
/// 1. Store generated artifacts (like optimized images or compiled CSS) to the `dist` directory.
/// 2. Register module imports (for the Import Map) that this task introduces.
///
/// It handles content-addressable storage (hashing) automatically to ensure caching works correctly.
#[derive(Clone)]
pub struct Runtime {
    pub(crate) new_imports: ImportMap,
}

impl Runtime {
    /// Creates a new, empty Runtime.
    pub fn new() -> Self {
        Self {
            new_imports: ImportMap::new(),
        }
    }

    /// Stores raw data as a content-addressed artifact.
    ///
    /// The data is hashed, and the file is stored at `/hash/<hash>.<ext>`.
    ///
    /// # Arguments
    ///
    /// * `data` - The raw bytes to store.
    /// * `ext` - The file extension for the stored file (e.g., "png", "css").
    ///
    /// # Returns
    ///
    /// The logical path to the file (e.g., `/hash/abcdef123.png`), suitable for use in HTML `src` attributes.
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

    /// Registers a new entry in the global Import Map.
    ///
    /// This tells the browser how to resolve a specific module specifier.
    ///
    /// # Arguments
    ///
    /// * `key` - The module specifier (e.g., "react", "my-lib").
    /// * `value` - The URL to the module (e.g., "/hash/1234.js", "https://cdn.example.com/lib.js").
    pub fn register(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.new_imports.register(key, value);
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

type GlobCallback<G, R> =
    Box<dyn Fn(&Context<G>, &mut Runtime, File) -> anyhow::Result<(Utf8PathBuf, R)> + Send + Sync>;

/// A task that finds files matching a glob pattern and processes them in parallel.
///
/// This is the implementation behind helper methods like `load_frontmatter` and `load_images`.
/// It is generic over the global context `G` and the result type `R`.
pub struct GlobRegistryTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    glob_entry: Vec<&'static str>,
    glob_watch: Vec<Pattern>,
    callback: GlobCallback<G, R>,
}

impl<G, R> GlobRegistryTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    /// Creates a new `GlobRegistryTask`.
    ///
    /// # Arguments
    ///
    /// * `glob_entry` - Patterns to search for files to process.
    /// * `glob_watch` - Patterns to watch for changes (retriggering the task).
    /// * `callback` - A function that processes each found file.
    pub fn new<F>(
        glob_entry: Vec<&'static str>,
        glob_watch: Vec<&'static str>,
        callback: F,
    ) -> Result<Self, HauchiwaError>
    where
        F: Fn(&Context<G>, &mut Runtime, File) -> anyhow::Result<(Utf8PathBuf, R)>
            + Send
            + Sync
            + 'static,
    {
        Ok(Self {
            glob_entry: glob_entry.to_vec(),
            glob_watch: glob_watch
                .into_iter()
                .map(Pattern::new)
                .collect::<Result<_, _>>()?,
            callback: Box::new(callback),
        })
    }
}

impl<G, R> TypedTask<G> for GlobRegistryTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    type Output = Registry<R>;

    fn get_name(&self) -> String {
        self.glob_entry.join(", ")
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn execute(
        &self,
        context: &Context<G>,
        runtime: &mut Runtime,
        _: &[Dynamic],
    ) -> anyhow::Result<Self::Output> {
        let mut paths = Vec::new();
        for glob_entry in &self.glob_entry {
            for path in glob(glob_entry)? {
                // Handle glob errors immediately here
                paths.push(Utf8PathBuf::try_from(path?)?);
            }
        }

        let results: anyhow::Result<Vec<_>> = paths
            .into_par_iter()
            .map(|path| {
                let data = fs::read(&path)?.into();
                let file = File { path, data };

                let mut rt = Runtime::new();

                // Call the user callback
                let (out_path, res) = (self.callback)(context, &mut rt, file)?;

                Ok((out_path, res, rt.new_imports))
            })
            .collect();

        let mut registry = HashMap::new();
        for (path, res, imports) in results? {
            registry.insert(path, res);
            runtime.new_imports.merge(imports);
        }

        let registry = Registry { map: registry };

        Ok(registry)
    }

    fn is_dirty(&self, path: &Utf8Path) -> bool {
        self.glob_watch.iter().any(|p| p.matches(path.as_str()))
    }
}

/// Generate the functions used to initialize content files. These functions can
/// be used to parse the front matter using engines from crate `gray_matter`.
macro_rules! matter_parser {
	($name:ident, $engine:path) => {
		#[doc = concat!(
			"This function can be used to extract metadata from a document with `D` as the frontmatter shape.\n",
			"Configured to use [`", stringify!($engine), "`] as the engine of the parser."
		)]
		fn $name<D>(content: &str) -> Result<(D, String), anyhow::Error>
		where
			D: for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
		{
		    use gray_matter::{Matter, Pod};

			// We can cache the creation of the parser
			static PARSER: std::sync::LazyLock<Matter<$engine>> = std::sync::LazyLock::new(Matter::<$engine>::new);

			let entity = PARSER.parse(content)?;
            let object = entity
                .data
                .unwrap_or_else(Pod::new_hash)
                .deserialize::<D>()
                .map_err(|e| anyhow::anyhow!("Malformed frontmatter:\n{e}"))?;

			Ok((
				// Just the front matter
				object,
				// The rest of the content
				entity.content,
			))
		}
	};
}

matter_parser!(parse_yaml, YAML);
// matter_parser!(parse_json, JSON);
