use std::{
    any::{TypeId, type_name},
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    Hash32, Input, InputItem,
    plugin::{Loadable, Runtime},
};

pub(crate) struct LoaderGeneric<T, R>
where
    T: 'static + Send + Sync,
    R: 'static + Send + Sync,
{
    path_base: &'static str,
    path_glob: &'static str,
    cached: HashMap<Utf8PathBuf, InputItem>,
    f1: fn(&Utf8Path) -> (Hash32, T),
    f2: fn(Runtime, T) -> R,
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
        f1: fn(&Utf8Path) -> (Hash32, T),
        f2: fn(Runtime, T) -> R,
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
}

impl<T, R> Loadable for LoaderGeneric<T, R>
where
    T: 'static + Send + Sync,
    R: 'static + Send + Sync,
{
    fn load(&mut self) {
        let path_base = self.path_base;
        let path_glob = self.path_glob;
        let cached = &mut self.cached;
        let f1 = self.f1;
        let f2 = self.f2;

        let pattern = Utf8Path::new(path_base).join(path_glob);
        let iter = glob::glob(pattern.as_str()).unwrap();

        let mut arr = vec![];
        for entry in iter {
            let path = Utf8PathBuf::try_from(entry.unwrap()).unwrap();
            arr.push(path);
        }

        if arr.is_empty() {
            return;
        }

        for file_path in arr {
            let (hash, data) = f1(&file_path);
            cached.insert(
                file_path.to_owned(),
                InputItem {
                    refl_type: TypeId::of::<R>(),
                    refl_name: type_name::<R>(),
                    slug: file_path.clone(),
                    file: file_path.clone(),
                    hash,
                    data: {
                        let rt = self.rt.clone();
                        Input::Lazy(LazyLock::new(Box::new(move || Arc::new(f2(rt, data)))))
                    },
                    info: None,
                },
            );
        }
    }

    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> bool {
        let path_base = self.path_base;
        let path_glob = self.path_glob;
        let cached = &mut self.cached;
        let f1 = self.f1;
        let f2 = self.f2;

        let pattern = Utf8Path::new(path_base).join(path_glob);
        let matcher = glob::Pattern::new(pattern.as_str()).unwrap();
        let mut changed = false;

        for entry in set {
            if !matcher.matches_path(entry.as_std_path()) {
                continue;
            }

            let (hash, data) = f1(entry);
            cached.insert(
                entry.to_owned(),
                InputItem {
                    refl_type: TypeId::of::<R>(),
                    refl_name: type_name::<R>(),
                    hash,
                    file: entry.to_owned(),
                    slug: entry.strip_prefix(path_base).unwrap_or(entry).to_owned(),
                    data: {
                        let rt = self.rt.clone();
                        Input::Lazy(LazyLock::new(Box::new(move || Arc::new(f2(rt, data)))))
                    },
                    info: None,
                },
            );
            changed = true;
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

pub(crate) struct LoaderGenericMultifile<T, R>
where
    T: 'static + Send + Sync,
    R: 'static + Send + Sync,
{
    path_base: &'static str,
    path_glob: &'static str,
    cached: HashMap<Utf8PathBuf, InputItem>,
    f1: fn(&Utf8Path) -> (Hash32, T),
    f2: fn(Runtime, T) -> R,
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
        f1: fn(&Utf8Path) -> (Hash32, T),
        f2: fn(Runtime, T) -> R,
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
}

impl<T, R> Loadable for LoaderGenericMultifile<T, R>
where
    T: 'static + Send + Sync,
    R: 'static + Send + Sync,
{
    fn load(&mut self) {
        let path_base = self.path_base;
        let path_glob = self.path_glob;
        let cached = &mut self.cached;
        let f1 = self.f1;
        let f2 = self.f2;

        let pattern = Utf8Path::new(path_base).join(path_glob);
        let iter = glob::glob(pattern.as_str()).unwrap();

        let mut arr = vec![];
        for entry in iter {
            let path = Utf8PathBuf::try_from(entry.unwrap()).unwrap();
            arr.push(path);
        }

        if arr.is_empty() {
            return;
        }

        for file_path in arr {
            let (hash, data) = f1(&file_path);
            cached.insert(
                file_path.to_owned(),
                InputItem {
                    refl_type: TypeId::of::<R>(),
                    refl_name: type_name::<R>(),
                    slug: file_path.clone(),
                    file: file_path.clone(),
                    hash,
                    data: {
                        let rt = self.rt.clone();
                        Input::Lazy(LazyLock::new(Box::new(move || Arc::new(f2(rt, data)))))
                    },
                    info: None,
                },
            );
        }
    }

    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> bool {
        if set.iter().any(|path| path.starts_with(self.path_base)) {
            self.load();
            true
        } else {
            false
        }
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
