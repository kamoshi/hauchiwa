use std::{
    any::{TypeId, type_name},
    collections::{HashMap, HashSet},
    fs,
    sync::{Arc, LazyLock},
};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{FileData, FromFile, Hash32, Item, LoaderError, LoaderFileError, plugin::Loadable};

pub struct Content<T>
where
    T: Send + Sync + 'static,
{
    pub meta: T,
    pub text: String,
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
    // repo: GitRepo,
}

impl<T> LoaderContent<T>
where
    T: Send + Sync + 'static,
{
    pub(crate) fn new(
        path_base: &'static str,
        path_glob: &'static str,
        preload: fn(&str) -> Result<(T, String), anyhow::Error>,
    ) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self {
            path_base,
            path_glob,
            preload,
            cached: HashMap::new(),
            // repo: todo!(),
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
                    file: path,
                    slug,
                    area,
                    info: None,
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
