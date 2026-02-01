//! Loaders are tasks that ingest data from the filesystem or external sources.
//!
//! A "Loader" is typically a task with **zero dependencies** that reads files
//! matching a glob pattern, processes them (e.g., parsing frontmatter, resizing
//! images), and stores them in [`Tracker`](crate::Tracker) accessible at runtime.
//!
//! Loaders that require JavaScript execution (like Svelte) do not embed V8.
//! Instead, they act as orchestrators, spawning `deno` subprocesses to handle
//! the compilation. This keeps the Rust binary small and compilation times
//! fast, leveraging Deno's existing toolchain for transpilation.

pub mod generic;
pub use generic::Document;

#[cfg(feature = "image")]
pub mod image;
#[cfg(feature = "image")]
pub use image::Image;

#[cfg(feature = "grass")]
pub mod css;
#[cfg(feature = "grass")]
pub use css::Stylesheet;

pub mod js;
pub use js::Script;

pub mod svelte;
pub use svelte::Svelte;
use tracing_indicatif::span_ext::IndicatifSpanExt;

#[cfg(feature = "tokio")]
pub mod tokio;

#[cfg(feature = "pagefind")]
pub mod pagefind;

#[cfg(feature = "sitemap")]
pub mod sitemap;

use std::{collections::BTreeMap, fs};

use camino::{Utf8Path, Utf8PathBuf};
use glob::{Pattern, glob};
use gray_matter::engine::YAML;
use petgraph::graph::NodeIndex;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::core::{Dynamic, Hash32, Store, TaskContext};
use crate::engine::{Map, Provenance, TypedFine};
use crate::error::HauchiwaError;

/// A raw file read from the filesystem.
///
/// This struct is passed to the callback of custom loaders.
pub struct Input {
    /// The path to the source file.
    pub path: Utf8PathBuf,
    /// The hash of the file content.
    pub(crate) hash: Hash32,
}

impl Input {
    /// Reads the file content from the filesystem.
    pub fn read(&self) -> std::io::Result<Box<[u8]>> {
        fs::read(&self.path).map(Into::into)
    }
}

type GlobFilesCallback<G, R> = Box<
    dyn Fn(&TaskContext<G>, &mut Store, Input) -> anyhow::Result<(Utf8PathBuf, R)> + Send + Sync,
>;

/// A task that finds files matching a glob pattern and processes them in parallel.
///
/// This is the implementation behind helper methods like `load_frontmatter` and `load_images`.
/// It is generic over the global context `G` and the result type `R`.
pub(crate) struct GlobFiles<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    glob_entry: Vec<String>,
    glob_watch: Vec<Pattern>,
    callback: GlobFilesCallback<G, R>,
}

impl<G, R> GlobFiles<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    /// Creates a new `GlobAssetsTask`.
    ///
    /// # Arguments
    ///
    /// * `glob_entry` - Patterns to search for files to process.
    /// * `glob_watch` - Patterns to watch for changes (retriggering the task).
    /// * `callback` - A function that processes each found file.
    pub fn new<F>(
        glob_entry: Vec<String>,
        glob_watch: Vec<String>,
        callback: F,
    ) -> Result<Self, HauchiwaError>
    where
        F: Fn(&TaskContext<G>, &mut Store, Input) -> anyhow::Result<(Utf8PathBuf, R)>
            + Send
            + Sync
            + 'static,
    {
        Ok(Self {
            glob_entry,
            glob_watch: glob_watch
                .iter()
                .map(|p| Pattern::new(p))
                .collect::<Result<_, _>>()?,
            callback: Box::new(callback),
        })
    }
}

impl<G, R> TypedFine<G> for GlobFiles<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    type Output = R;

    fn get_name(&self) -> String {
        self.glob_entry.join(", ")
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn get_watched(&self) -> Vec<camino::Utf8PathBuf> {
        self.glob_watch
            .iter()
            .map(|pat| Utf8PathBuf::from(pat.as_str()))
            .collect()
    }

    fn execute(
        &self,
        context: &TaskContext<G>,
        runtime: &mut Store,
        _: &[Dynamic],
    ) -> anyhow::Result<Map<Self::Output>> {
        let mut paths = Vec::new();
        for glob_entry in &self.glob_entry {
            for path in glob(glob_entry)? {
                // Handle glob errors immediately here
                paths.push(Utf8PathBuf::try_from(path?)?);
            }
        }

        // we can override the style to have progress
        let style = crate::utils::get_style_task_progress()?;
        context.span.pb_set_style(&style);
        context.span.pb_set_length(paths.len() as u64);

        let results: anyhow::Result<Vec<_>> = paths
            .into_par_iter()
            .map(|path| {
                let hash = Hash32::hash_file(&path)?;
                let file = Input { path, hash };

                let mut rt = Store::new();

                // call the user callback
                let (path, res) = (self.callback)(context, &mut rt, file)?;

                // next iteration
                context.span.pb_inc(1);

                Ok((Provenance(hash), path, res, rt.imports))
            })
            .collect();

        let mut registry = BTreeMap::new();
        for (provenance, path, res, imports) in results? {
            registry.insert(path.into_string(), (res, provenance));
            runtime.imports.merge(imports);
        }

        Ok(Map { map: registry })
    }

    fn is_dirty(&self, path: &Utf8Path) -> bool {
        self.glob_watch.iter().any(|p| p.matches(path.as_str()))
    }
}

type GlobBundleCallback<G, R> = Box<
    dyn Fn(&TaskContext<G>, &mut Store, Input) -> anyhow::Result<(Hash32, Utf8PathBuf, R)>
        + Send
        + Sync,
>;

/// A task that finds files matching a glob pattern and processes them in parallel.
///
/// This is the implementation behind helper methods like `load_frontmatter` and `load_images`.
/// It is generic over the global context `G` and the result type `R`.
pub(crate) struct GlobBundle<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    glob_entry: Vec<String>,
    glob_watch: Vec<Pattern>,
    callback: GlobBundleCallback<G, R>,
}

impl<G, R> GlobBundle<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    /// Creates a new `GlobAssetsTask`.
    ///
    /// # Arguments
    ///
    /// * `glob_entry` - Patterns to search for files to process.
    /// * `glob_watch` - Patterns to watch for changes (retriggering the task).
    /// * `callback` - A function that processes each found file.
    pub(crate) fn new<F>(
        glob_entry: Vec<String>,
        glob_watch: Vec<String>,
        callback: F,
    ) -> Result<Self, HauchiwaError>
    where
        F: Fn(&TaskContext<G>, &mut Store, Input) -> anyhow::Result<(Hash32, Utf8PathBuf, R)>
            + Send
            + Sync
            + 'static,
    {
        Ok(Self {
            glob_entry,
            glob_watch: glob_watch
                .iter()
                .map(|p| Pattern::new(p))
                .collect::<Result<_, _>>()?,
            callback: Box::new(callback),
        })
    }
}

impl<G, R> TypedFine<G> for GlobBundle<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    type Output = R;

    fn get_name(&self) -> String {
        self.glob_entry.join(", ")
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn get_watched(&self) -> Vec<camino::Utf8PathBuf> {
        self.glob_watch
            .iter()
            .map(|pat| Utf8PathBuf::from(pat.as_str()))
            .collect()
    }

    fn execute(
        &self,
        context: &TaskContext<G>,
        runtime: &mut Store,
        _: &[Dynamic],
    ) -> anyhow::Result<Map<Self::Output>> {
        let mut paths = Vec::new();
        for glob_entry in &self.glob_entry {
            for path in glob(glob_entry)? {
                // Handle glob errors immediately here
                paths.push(Utf8PathBuf::try_from(path?)?);
            }
        }

        // we can override the style to have progress
        let style = crate::utils::get_style_task_progress()?;
        context.span.pb_set_style(&style);
        context.span.pb_set_length(paths.len() as u64);

        let results: anyhow::Result<Vec<_>> = paths
            .into_par_iter()
            .map(|path| {
                let hash = Hash32::hash_file(&path)?;
                let file = Input { path, hash };

                let mut rt = Store::new();

                // call the user callback
                let (hash, path, res) = (self.callback)(context, &mut rt, file)?;

                // next iteration
                context.span.pb_inc(1);

                Ok((Provenance(hash), path, res, rt.imports))
            })
            .collect();

        let mut registry = BTreeMap::new();
        for (provenance, path, res, imports) in results? {
            registry.insert(path.into_string(), (res, provenance));
            runtime.imports.merge(imports);
        }

        Ok(Map { map: registry })
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
