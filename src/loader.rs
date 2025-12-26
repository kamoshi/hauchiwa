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

/// A collection of processed assets, mapping source file paths to their
/// resulting data.
///
/// `Registry` is a common return type for loader tasks that process multiple
/// files, such as `load` or `load_frontmatter`. It provides a way to access the
/// processed output of each file by its original path.
#[derive(Debug)]
pub struct Registry<T> {
    map: HashMap<camino::Utf8PathBuf, T>,
}

impl<T: Clone> Registry<T> {
    /// Retrieves a reference to the processed data for a given source path.
    pub fn get(&self, path: impl AsRef<Utf8Path>) -> Result<&T, HauchiwaError> {
        self.map
            .get(path.as_ref())
            .ok_or(HauchiwaError::AssetNotFound(
                path.as_ref().to_string().into(),
            ))
    }

    /// Returns an iterator over the processed data of all files in the registry.
    pub fn values(&self) -> std::collections::hash_map::Values<'_, Utf8PathBuf, T> {
        self.map.values()
    }

    /// Finds all assets whose paths match the given glob pattern.
    /// Returns a vector of (Path, Value) tuples.
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

/// Represents a source file that is being processed by a loader.
///
/// This struct provides loaders with the file's path and its raw content or metadata,
/// enabling tasks to perform operations like parsing, transformation, or analysis.
///
/// `Runtime` abstracts filesystem interactions related to build artifact
pub struct File<T> {
    /// The path to the source file.
    pub path: Utf8PathBuf,
    /// The metadata or content of the file.
    pub metadata: T,
}

/// storage, enabling immutability and reproducibility guarantees through
/// content hashing. Also handles import map registration.
#[derive(Clone)]
pub struct Runtime {
    pub(crate) new_imports: ImportMap,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            new_imports: ImportMap::new(),
        }
    }

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

    /// Register a new module key and its path into the import map for this task.
    ///
    /// # Arguments
    /// * `key` - The module specifier (e.g., "svelte")
    /// * `value` - The URL or path (e.g., "/_app/svelte.js")
    pub fn register(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.new_imports.register(key, value);
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

type GlobCallback<G, R> = Box<
    dyn Fn(&Context<G>, &mut Runtime, File<Vec<u8>>) -> anyhow::Result<(Utf8PathBuf, R)>
        + Send
        + Sync,
>;

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
    pub fn new<F>(
        glob_entry: Vec<&'static str>,
        glob_watch: Vec<&'static str>,
        callback: F,
    ) -> Result<Self, HauchiwaError>
    where
        F: Fn(&Context<G>, &mut Runtime, File<Vec<u8>>) -> anyhow::Result<(Utf8PathBuf, R)>
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
                let data = fs::read(&path)?;

                let file = File {
                    path,
                    metadata: data,
                };

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
