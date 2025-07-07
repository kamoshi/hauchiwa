use std::{
    any::{Any, TypeId, type_name},
    collections::{HashMap, HashSet},
    fs,
    sync::Arc,
};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    Hash32, Input, InputContent, InputItem, LoaderError, LoaderFileCallbackError, LoaderFileError,
    plugin::Loadable,
};

type ArcAny = Arc<dyn Any + Send + Sync>;

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

pub struct LoaderContent {
    refl_type: TypeId,
    refl_name: &'static str,
    path_base: &'static str,
    path_glob: &'static str,
    init: ContentFn,
    /// Content loaded and saved between multiple loads, cached by file path. We
    /// can check the hash of the item against file to see whether it changed.
    cached: HashMap<Utf8PathBuf, InputItem>,
    // repo: GitRepo,
}

impl LoaderContent {
    pub(crate) fn new<T>(
        path_base: &'static str,
        path_glob: &'static str,
        parse_matter: fn(&str) -> Result<(T, String), anyhow::Error>,
    ) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self {
            refl_type: TypeId::of::<T>(),
            refl_name: type_name::<T>(),
            path_base,
            path_glob,
            init: ContentFn::new(parse_matter),
            cached: HashMap::new(),
            // repo: todo!(),
        }
    }

    /// Helper function, convert file into InputItem
    /// TODO: based on loader cache, here we can use Hash32 to check if the
    /// previously loaded content item already exists, and *if* we have it, we
    /// can skip the `init.call`, because we can just reuse the old one.
    fn read_file(&self, path: Utf8PathBuf) -> Result<Option<InputItem>, LoaderFileError> {
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

        let slug = area
            .strip_prefix(self.path_base)
            .unwrap_or(&path)
            .to_owned();

        Ok(Some(InputItem {
            refl_type: self.refl_type,
            refl_name: self.refl_name,
            hash,
            info: None, //repo.files.get(path.as_str()).cloned(),
            file: path,
            slug,
            data: Input::Content(InputContent { area, meta, text }),
        }))
    }
}

impl Loadable for LoaderContent {
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
            self.cached.insert(item.file.clone(), item);
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
                self.cached.insert(item.file.clone(), item);
                changed |= true;
            }
        }

        changed
    }

    fn items(&self) -> Vec<&crate::InputItem> {
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
