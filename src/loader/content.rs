use std::{
    any::{TypeId, type_name},
    collections::{HashMap, HashSet},
    fs,
    sync::{Arc, LazyLock},
};

use camino::{Utf8Path, Utf8PathBuf};
use gray_matter::engine::{JSON, YAML};

use crate::{
    FileData, FromFile, GitRepo, Hash32, Item, Loader, LoaderError, LoaderFileError,
    loader::Loadable,
};

pub struct Content<T>
where
    T: Send + Sync + 'static,
{
    pub meta: T,
    pub text: String,
}

/// Create a new collection which draws content from the filesystem files
/// via a glob pattern. Usually used to collect articles written as markdown
/// files, however it is completely format agnostic.
///
/// The parameter `parse_matter` allows you to customize how the metadata
/// should be parsed. Default functions for the most common formats are
/// provided by library:
/// * [`parse_matter_json`](`crate::parse_matter_json`) - parse JSON metadata
/// * [`parse_matter_yaml`](`crate::parse_matter_yaml`) - parse YAML metadata
///
/// # Examples
///
/// ```rust
/// Collection::glob_with("content", "posts/**/*", ["md"], parse_matter_yaml::<Post>);
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

    /// Helper function, convert file into InputItem
    /// TODO: based on loader cache, here we can use Hash32 to check if the
    /// previously loaded content item already exists, and *if* we have it, we
    /// can skip the `init.call`, because we can just reuse the old one.
    fn read_file(&self, path: Utf8PathBuf) -> Result<Option<Item>, LoaderFileError> {
        if path.is_dir() {
            return Ok(None);
        }

        let bytes = fs::read(&path)?;
        let _hash = Hash32::hash(&bytes);

        let area = match path.file_stem() {
            Some("index") => path
                .parent()
                .map(ToOwned::to_owned)
                .unwrap_or(path.with_extension("")),
            _ => path.with_extension(""),
        };

        let slug = area
            .strip_prefix(self.path_base)
            .unwrap_or(&path)
            .to_owned();

        Ok(Some(Item {
            refl_type: TypeId::of::<Content<T>>(),
            refl_name: type_name::<Content<T>>(),
            // hash,
            data: FromFile {
                file: Arc::new(FileData {
                    info: self
                        .repo
                        .as_deref()
                        .and_then(|repo| repo.files.get(path.as_str()).cloned()),
                    file: path,
                    slug,
                    area,
                }),
                data: {
                    let preload = self.preload;
                    LazyLock::new(Box::new(move || {
                        let text = String::from_utf8_lossy(&bytes);
                        let (meta, text) = preload(&text).unwrap();
                        Arc::new(Content { meta, text })
                    }))
                },
            },
        }))
    }
}

impl<T> Loadable for LoaderContent<T>
where
    T: Send + Sync + 'static,
{
    fn load(&mut self) {
        let pattern = Utf8Path::new(self.path_base).join(self.path_glob);

        let mut vec = vec![];
        for path in glob::glob(pattern.as_str()).unwrap() {
            let path = Utf8PathBuf::try_from(path.unwrap()).unwrap();

            if let Some(item) = self
                .read_file(path.clone())
                .map_err(|e| LoaderError::LoaderGlobFile(path, e))
                .unwrap()
            {
                vec.push(item);
            }
        }

        for item in vec {
            self.cached.insert(item.data.file.file.clone(), item);
        }
    }

    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> bool {
        let pattern = Utf8Path::new(self.path_base).join(self.path_glob);
        let pattern = glob::Pattern::new(pattern.as_str()).unwrap();
        let mut changed = false;

        for path in set {
            if !pattern.matches_path(path.as_std_path()) {
                continue;
            };

            if let Some(item) = self
                .read_file(path.clone())
                .map_err(|e| LoaderError::LoaderGlobFile(path.to_owned(), e))
                .unwrap()
            {
                self.cached.insert(item.data.file.file.clone(), item);
                changed |= true;
            }
        }

        changed
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
