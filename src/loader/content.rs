use std::any::{TypeId, type_name};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::fs;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};

use crate::{ArcAny, GitRepo, Hash32, Input, InputContent, InputItem, error::*};

type ContentFnPtr =
    Arc<dyn Fn(&str) -> Result<(ArcAny, String), LoaderFileCallbackError> + Send + Sync>;

#[derive(Clone)]
struct ContentFn(ContentFnPtr);

impl ContentFn {
    fn new<D>(parse_matter: fn(&str) -> Result<(D, String), anyhow::Error>) -> Self
    where
        D: Send + Sync + 'static,
    {
        Self(Arc::new(move |content| {
            let (meta, data) = parse_matter(content).map_err(LoaderFileCallbackError)?;
            Ok((Arc::new(meta), data))
        }))
    }

    fn call(&self, data: &str) -> Result<(ArcAny, String), LoaderFileCallbackError> {
        (self.0)(data)
    }
}

impl Debug for ContentFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InitFn(*)")
    }
}

/// How to load data?
enum ContentStrategy {
    Glob(LoaderGlob),
}

struct LoaderGlob {
    refl_type: TypeId,
    refl_name: &'static str,
    base: &'static str,
    glob: &'static str,
    init: ContentFn,
    /// Content loaded and saved between multiple loads, cached by file path. We
    /// can check the hash of the item against file to see whether it changed.
    cached: HashMap<Utf8PathBuf, InputItem>,
}

impl LoaderGlob {
    /// Read many files into InputItem
    fn load(&mut self, repo: &GitRepo) -> Result<(), LoaderError> {
        let pattern = Utf8Path::new(self.base).join(self.glob);

        let mut vec = vec![];
        for path in glob::glob(pattern.as_str())? {
            let path = Utf8PathBuf::try_from(path?)?;

            if let Some(item) = self
                .read_file(path.clone(), repo)
                .map_err(|e| LoaderError::LoaderGlobFile(path, e))?
            {
                vec.push(item);
            }
        }

        for item in vec {
            self.cached.insert(item.file.clone(), item);
        }

        Ok(())
    }

    fn reload(&mut self, set: &HashSet<Utf8PathBuf>, repo: &GitRepo) -> Result<bool, LoaderError> {
        let pattern = Utf8Path::new(self.base).join(self.glob);
        let pattern = glob::Pattern::new(pattern.as_str()).unwrap();
        let mut changed = false;

        for path in set {
            if !pattern.matches_path(path.as_std_path()) {
                continue;
            };

            if let Some(item) = self
                .read_file(path.clone(), repo)
                .map_err(|e| LoaderError::LoaderGlobFile(path.to_owned(), e))?
            {
                self.cached.insert(item.file.clone(), item);
                changed |= true;
            }
        }

        Ok(changed)
    }

    /// Helper function, convert file into InputItem
    /// TODO: based on loader cache, here we can use Hash32 to check if the
    /// previously loaded content item already exists, and *if* we have it, we
    /// can skip the `init.call`, because we can just reuse the old one.
    fn read_file(
        &self,
        path: Utf8PathBuf,
        repo: &GitRepo,
    ) -> Result<Option<InputItem>, LoaderFileError> {
        if path.is_dir() {
            return Ok(None);
        }

        let bytes = fs::read(&path)?;
        let hash = Hash32::hash(&bytes);
        let text = String::from_utf8_lossy(&bytes);
        let (meta, text) = self.init.call(&text)?;

        let area = match path.file_stem() {
            Some("index") => path
                .parent()
                .map(ToOwned::to_owned)
                .unwrap_or(path.with_extension("")),
            _ => path.with_extension(""),
        };

        let slug = area.strip_prefix(self.base).unwrap_or(&path).to_owned();

        Ok(Some(InputItem {
            refl_type: self.refl_type,
            refl_name: self.refl_name,
            hash,
            info: repo.files.get(path.as_str()).cloned(),
            file: path,
            slug,
            data: Input::Content(InputContent { area, meta, text }),
        }))
    }
}

/// An opaque representation of a source of inputs loaded into the generator.
/// You can think of a single collection as a set of written articles with
/// shared frontmatter shape, for example your blog posts.
///
/// Hovewer, a collection can also load additional files like images or custom
/// assets. This is useful when you want to colocate assets and images next to
/// the articles. A common use case is to directly reference the images relative
/// to the markdown file.
pub struct Content {
    /// Data loading strategy
    loader: ContentStrategy,
}

impl Content {
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
    pub fn glob<D>(
        path_base: &'static str,
        path_glob: &'static str,
        parse_matter: fn(&str) -> Result<(D, String), anyhow::Error>,
    ) -> Self
    where
        D: Send + Sync + 'static,
    {
        Self {
            loader: ContentStrategy::Glob(LoaderGlob {
                refl_type: TypeId::of::<D>(),
                refl_name: type_name::<D>(),
                base: path_base,
                glob: path_glob,
                init: ContentFn::new(parse_matter),
                cached: HashMap::new(),
            }),
        }
    }

    pub(crate) fn load(&mut self, repo: &GitRepo) -> Result<(), LoaderError> {
        match &mut self.loader {
            ContentStrategy::Glob(glob) => glob.load(repo)?,
        };

        Ok(())
    }

    pub(crate) fn remove(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool {
        match &mut self.loader {
            ContentStrategy::Glob(loader) => {
                let before = loader.cached.len();
                loader.cached.retain(|path, _| !obsolete.contains(path));
                loader.cached.len() < before
            }
        }
    }

    pub(crate) fn reload(
        &mut self,
        set: &HashSet<Utf8PathBuf>,
        repo: &GitRepo,
    ) -> Result<bool, LoaderError> {
        match &mut self.loader {
            ContentStrategy::Glob(loader) => loader.reload(set, repo),
        }
    }

    pub(crate) fn items(&self) -> Vec<&InputItem> {
        match &self.loader {
            ContentStrategy::Glob(glob) => glob.cached.values().collect(),
        }
    }

    pub(crate) fn path_base(&self) -> &'static str {
        match &self.loader {
            ContentStrategy::Glob(glob) => glob.base,
        }
    }
}
