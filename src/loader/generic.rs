use std::{
    any::{TypeId, type_name},
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    FileData, FromFile, Hash32, Item, LoaderError,
    loader::{Loadable, Runtime},
};

pub(crate) struct LoaderGeneric<T, R>
where
    T: 'static + Send + Sync,
    R: 'static + Send + Sync,
{
    path_base: &'static str,
    path_glob: &'static str,
    cached: HashMap<Utf8PathBuf, Item>,
    f1: fn(&Utf8Path) -> anyhow::Result<(Hash32, T)>,
    f2: fn(Runtime, T) -> anyhow::Result<R>,
    rt: Runtime,
}

impl<T, R> LoaderGeneric<T, R>
where
    T: 'static + Send + Sync,
    R: 'static + Send + Sync,
{
    pub fn new(
        path_base: &'static str,
        path_glob: &'static str,
        f1: fn(&Utf8Path) -> anyhow::Result<(Hash32, T)>,
        f2: fn(Runtime, T) -> anyhow::Result<R>,
    ) -> Self {
        Self {
            path_base,
            path_glob,
            cached: HashMap::new(),
            f1,
            f2,
            rt: Runtime,
        }
    }

    fn check_loaded(&self, path: &Utf8Path, hash: Hash32) -> bool {
        match self.cached.get(path) {
            Some(item) => item.hash == hash,
            None => false,
        }
    }
}

impl<T, R> Loadable for LoaderGeneric<T, R>
where
    T: 'static + Send + Sync,
    R: 'static + Send + Sync,
{
    fn name(&self) -> Cow<'static, str> {
        Utf8Path::new(self.path_base)
            .join(self.path_glob)
            .to_string()
            .into()
    }

    fn load(&mut self) -> Result<(), LoaderError> {
        let path_base = self.path_base;
        let path_glob = self.path_glob;
        let f1 = self.f1;
        let f2 = self.f2;

        let pattern = Utf8Path::new(path_base).join(path_glob);
        let iter = glob::glob(pattern.as_str())?;

        let mut arr = vec![];
        for entry in iter {
            let path = Utf8PathBuf::try_from(entry?)?;
            arr.push(path);
        }

        if arr.is_empty() {
            return Ok(());
        }

        for file in arr {
            let area = file.with_extension("");
            let (hash, data) = f1(&file)?;
            if self.check_loaded(&file, hash) {
                continue;
            }

            self.cached.insert(
                file.to_owned(),
                Item {
                    refl_type: TypeId::of::<R>(),
                    refl_name: type_name::<R>(),
                    id: file.as_str().into(),
                    hash,
                    data: FromFile {
                        file: Arc::new(FileData {
                            file: file.clone(),
                            slug: file.clone(),
                            area,
                            info: None,
                        }),
                        data: {
                            let rt = self.rt.clone();
                            LazyLock::new(Box::new(move || Ok(Arc::new(f2(rt, data)?))))
                        },
                    },
                },
            );
        }

        Ok(())
    }

    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> Result<bool, LoaderError> {
        let path_base = self.path_base;
        let path_glob = self.path_glob;
        let f1 = self.f1;
        let f2 = self.f2;

        let pattern = Utf8Path::new(path_base).join(path_glob);
        let matcher = glob::Pattern::new(pattern.as_str())?;
        let mut changed = false;

        for file in set {
            if !matcher.matches_path(file.as_std_path()) {
                continue;
            }

            let area = file.with_extension("");
            let (hash, data) = f1(file)?;
            if self.check_loaded(file, hash) {
                continue;
            }

            self.cached.insert(
                file.to_owned(),
                Item {
                    refl_type: TypeId::of::<R>(),
                    refl_name: type_name::<R>(),
                    id: file.as_str().into(),
                    hash,
                    data: FromFile {
                        file: Arc::new(FileData {
                            file: file.clone(),
                            slug: file.clone(),
                            area,
                            info: None,
                        }),
                        data: {
                            let rt = self.rt.clone();
                            LazyLock::new(Box::new(move || Ok(Arc::new(f2(rt, data)?))))
                        },
                    },
                },
            );
            changed = true;
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

pub(crate) struct LoaderGenericMultifile<T, R>
where
    T: 'static + Send + Sync,
    R: 'static + Send + Sync,
{
    path_base: &'static str,
    path_glob: &'static str,
    cached: HashMap<Utf8PathBuf, Item>,
    f1: fn(&Utf8Path) -> anyhow::Result<(Hash32, T)>,
    f2: fn(Runtime, T) -> anyhow::Result<R>,
    rt: Runtime,
}

impl<T, R> LoaderGenericMultifile<T, R>
where
    T: 'static + Send + Sync,
    R: 'static + Send + Sync,
{
    pub fn new(
        path_base: &'static str,
        path_glob: &'static str,
        f1: fn(&Utf8Path) -> anyhow::Result<(Hash32, T)>,
        f2: fn(Runtime, T) -> anyhow::Result<R>,
    ) -> Self {
        Self {
            path_base,
            path_glob,
            cached: HashMap::new(),
            f1,
            f2,
            rt: Runtime,
        }
    }

    fn check_loaded(&self, path: &Utf8Path, hash: Hash32) -> bool {
        match self.cached.get(path) {
            Some(item) => item.hash == hash,
            None => false,
        }
    }
}

impl<T, R> Loadable for LoaderGenericMultifile<T, R>
where
    T: 'static + Send + Sync,
    R: 'static + Send + Sync,
{
    fn name(&self) -> Cow<'static, str> {
        Utf8Path::new(self.path_base)
            .join(self.path_glob)
            .to_string()
            .into()
    }

    fn load(&mut self) -> Result<(), LoaderError> {
        let path_base = self.path_base;
        let path_glob = self.path_glob;
        let f1 = self.f1;
        let f2 = self.f2;

        let pattern = Utf8Path::new(path_base).join(path_glob);
        let iter = glob::glob(pattern.as_str())?;

        let mut arr = vec![];
        for entry in iter {
            let path = Utf8PathBuf::try_from(entry?)?;
            arr.push(path);
        }

        if arr.is_empty() {
            return Ok(());
        }

        for file in arr {
            let area = file.with_extension("");
            let (hash, data) = f1(&file)?;
            if self.check_loaded(&file, hash) {
                continue;
            }

            self.cached.insert(
                file.to_owned(),
                Item {
                    refl_type: TypeId::of::<R>(),
                    refl_name: type_name::<R>(),
                    id: file.as_str().into(),
                    hash,
                    data: FromFile {
                        file: Arc::new(FileData {
                            file: file.clone(),
                            slug: file.clone(),
                            area,
                            info: None,
                        }),
                        data: {
                            let rt = self.rt.clone();
                            LazyLock::new(Box::new(move || Ok(Arc::new(f2(rt, data)?))))
                        },
                    },
                },
            );
        }

        Ok(())
    }

    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> Result<bool, LoaderError> {
        if set.iter().any(|path| path.starts_with(self.path_base)) {
            self.load()?;
            Ok(true)
        } else {
            Ok(false)
        }
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
