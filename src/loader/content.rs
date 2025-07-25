use std::{
    any::{TypeId, type_name},
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs,
    sync::{Arc, LazyLock},
};

use camino::{Utf8Path, Utf8PathBuf};
use gray_matter::engine::{JSON, YAML};

use crate::{
    FileData, GitRepo, Hash32, Item, LazyAssetError, Loader, LoaderError, loader::Loadable,
};

/// This is the canonical in-memory representation for markdown, or any textual
/// content files parsed via front matter. Used as the payload type for
/// [`glob_content`]-driven collections.
pub struct Content<T>
where
    T: Send + Sync + 'static,
{
    /// Deserialized front matter, typically JSON or YAML.
    pub meta: T,
    /// The raw document body, stripped of metadata.
    pub text: String,
}

/// Constructs a new [`Loader`] instance that ingests a collection of content files
/// matching a glob pattern and parses their front matter.
///
/// This function is format-agnostic: metadata parsing is delegated to the `preload`
/// function, which must return a `(meta, body)` tuple from the raw string. It is
/// commonly used for blog posts, documentation pages, or other page-like content.
///
/// By design, this loader memoizes its results keyed by content hash, and gracefully
/// handles partial reloads. If used with a git-backed repo, it can optionally include
/// VCS metadata in each [`FileData`] node.
///
/// # Parameters
/// - `path_base`: Root folder where content is stored, e.g. `"content"`.
/// - `path_glob`: Relative glob pattern, e.g. `"posts/**/*.md"`.
/// - `preload`: Function that parses the full content string and extracts metadata.
///
/// # Examples
///
/// ```rust
/// use hauchiwa::{Context, TaskResult, Page, loader::{Content, glob_content, yaml}};
///
/// type PostFrontMatter = ();
///
/// //loader
/// let loader = glob_content("content", "posts/**/*.md", yaml::<PostFrontMatter>);
///
/// // task
/// fn task(ctx: Context) -> TaskResult<Vec<Page>> {
///     let posts = ctx.glob_with_file::<Content<PostFrontMatter>>("posts/**/*")?;
///
///     // transform the content...
///
///     Ok(vec![])
/// }
/// ```
pub fn glob_content<T>(
    path_base: &'static str,
    path_glob: &'static str,
    preload: fn(&str) -> Result<(T, String), anyhow::Error>,
) -> Loader
where
    T: Send + Sync + 'static,
{
    Loader::with(move |init| LoaderContent::new(path_base, path_glob, preload, init.repo.clone()))
}

pub struct LoaderContent<T>
where
    T: Send + Sync + 'static,
{
    path_base: &'static str,
    path_glob: &'static str,
    preload: fn(&str) -> Result<(T, String), anyhow::Error>,
    /// Content loaded and saved between multiple loads, cached by file path. We
    /// can check the hash of the item against file to see whether it changed.
    cached: HashMap<Utf8PathBuf, Item>,
    repo: Option<Arc<GitRepo>>,
}

impl<T> LoaderContent<T>
where
    T: Send + Sync + 'static,
{
    pub(crate) fn new(
        path_base: &'static str,
        path_glob: &'static str,
        preload: fn(&str) -> Result<(T, String), anyhow::Error>,
        repo: Option<Arc<GitRepo>>,
    ) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self {
            path_base,
            path_glob,
            preload,
            cached: HashMap::new(),
            repo,
        }
    }

    fn check_loaded(&self, path: &Utf8Path, hash: Hash32) -> bool {
        match self.cached.get(path) {
            Some(item) => item.hash == hash,
            None => false,
        }
    }

    /// Helper function, convert file into InputItem
    fn read_file(&self, path: Utf8PathBuf) -> Result<Option<Item>, LoaderError> {
        if path.is_dir() {
            return Ok(None);
        }

        let bytes = fs::read(&path)?;
        let hash = Hash32::hash(&bytes);
        if self.check_loaded(&path, hash) {
            return Ok(None);
        }

        // Generally content files should be special-cased. The index files are
        // treated as if they were placed in parent directory.
        let is_index = matches!(path.file_stem(), Some("index"));
        let path_ext = path.extension();

        let path_rel = path.strip_prefix(self.path_base).unwrap_or(&path);
        let path_rel = if is_index {
            path_rel.parent().unwrap_or(path_rel)
        } else {
            path_rel
        };
        let path_rel = match path_ext {
            Some(ext) => path_rel.with_extension(ext),
            None => path_rel.to_path_buf(),
        };

        // Area should match the area of items colocated with this content item.
        let area = match path.file_stem() {
            Some("index") => path.parent().unwrap_or(&path),
            _ => &path,
        };
        let area = area.strip_prefix(self.path_base).unwrap_or(area);
        let area = area.with_extension("");

        Ok(Some(Item {
            refl_type: TypeId::of::<Content<T>>(),
            refl_name: type_name::<Content<T>>(),
            id: path_rel.into_string().into(),
            hash,
            data: {
                let preload = self.preload;
                LazyLock::new(Box::new(move || {
                    let text = String::from_utf8(bytes).map_err(LazyAssetError::new)?;
                    let (meta, text) = preload(&text).map_err(LazyAssetError::new)?;
                    Ok(Arc::new(Content { meta, text }))
                }))
            },
            file: Some(Arc::new(FileData {
                info: self
                    .repo
                    .as_deref()
                    .and_then(|repo| repo.files.get(path.as_str()).cloned()),
                file: path,
                area,
            })),
        }))
    }
}

impl<T> Loadable for LoaderContent<T>
where
    T: Send + Sync + 'static,
{
    fn name(&self) -> Cow<'static, str> {
        Utf8Path::new(self.path_base)
            .join(self.path_glob)
            .to_string()
            .into()
    }

    fn load(&mut self) -> Result<(), LoaderError> {
        let pattern = Utf8Path::new(self.path_base).join(self.path_glob);

        let mut vec = vec![];
        for path in glob::glob(pattern.as_str())? {
            let path = Utf8PathBuf::try_from(path?)?;

            if let Some(item) = self.read_file(path.clone())? {
                vec.push((path, item));
            }
        }

        for (path, item) in vec {
            self.cached.insert(path, item);
        }

        Ok(())
    }

    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> Result<bool, LoaderError> {
        let pattern = Utf8Path::new(self.path_base).join(self.path_glob);
        let pattern = glob::Pattern::new(pattern.as_str())?;
        let mut changed = false;

        for path in set {
            if !pattern.matches_path(path.as_std_path()) {
                continue;
            };

            if let Some(item) = self.read_file(path.clone())? {
                self.cached.insert(path.clone(), item);
                changed |= true;
            }
        }

        Ok(changed)
    }

    fn items(&self) -> Vec<&crate::Item> {
        self.cached.values().collect()
    }

    fn path_base(&self) -> &'static str {
        self.path_base
    }

    fn remove(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool {
        let before = self.cached.len();
        self.cached.retain(|path, _| !obsolete.contains(path));
        self.cached.len() < before
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
		pub fn $name<D>(content: &str) -> Result<(D, String), anyhow::Error>
		where
			D: for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
		{
		    use gray_matter::{Matter, Pod};

			// We can cache the creation of the parser
			static PARSER: LazyLock<Matter<$engine>> = LazyLock::new(Matter::<$engine>::new);

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

matter_parser!(yaml, YAML);
matter_parser!(json, JSON);
