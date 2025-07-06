use std::any::{TypeId, type_name};
use std::collections::HashSet;
use std::fmt::Debug;
use std::fs;
use std::sync::{LazyLock, RwLock};
use std::{collections::HashMap, sync::Arc};

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};

use crate::loader::{compile_esbuild, compile_svelte_html, compile_svelte_init};
use crate::runtime::{build_deferred, write_hashed_data};
use crate::{ArcAny, Hash32, Input, InputItem, Script, Stylesheet, Svelte};

type BoxFn8 = Box<dyn Fn(&[u8]) -> ArcAny + Send + Sync>;

struct AssetsGlobFn(BoxFn8);

impl Debug for AssetsGlobFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fn(*)")
    }
}

#[derive(Debug)]
pub(crate) struct BookkeepingDeferred {
    pub(crate) func: fn(&[u8]) -> Vec<u8>,
    pub(crate) done: RwLock<HashMap<Hash32, Hash32>>,
}

impl BookkeepingDeferred {
    fn new(func: fn(&[u8]) -> Vec<u8>) -> Self {
        Self {
            func,
            done: RwLock::new(HashMap::new()),
        }
    }

    pub(crate) fn read(&self, i: Hash32) -> Option<Hash32> {
        self.done.read().unwrap().get(&i).copied()
    }

    pub(crate) fn save(&self, i: Hash32, o: Hash32) {
        self.done.write().unwrap().insert(i, o);
    }
}

enum AssetsLoader {
    Glob {
        path_base: &'static str,
        path_glob: &'static str,
        func: AssetsGlobFn,
        cached: HashMap<Utf8PathBuf, InputItem>,
    },
    GlobDefer {
        path_base: &'static str,
        path_glob: &'static str,
        cached: HashMap<Utf8PathBuf, InputItem>,
        bookkeeping: Arc<BookkeepingDeferred>,
    },
    GlobScripts {
        path_base: &'static str,
        path_glob: &'static str,
        cached: HashMap<Utf8PathBuf, InputItem>,
    },
    GlobStyles {
        path_base: &'static str,
        path_glob: &'static str,
        cached: HashMap<Utf8PathBuf, InputItem>,
    },
    #[cfg(feature = "images")]
    DeferredTwoStage {
        path_base: &'static str,
        path_glob: &'static str,
        cached: HashMap<Utf8PathBuf, InputItem>,
    },
    GlobSvelte {
        path_base: &'static str,
        path_glob: &'static str,
        cached: HashMap<Utf8PathBuf, InputItem>,
    },
}

pub struct Assets {
    refl_type: TypeId,
    refl_name: &'static str,
    kind: AssetsLoader,
}

impl Assets {
    pub fn glob<T>(path_base: &'static str, path_glob: &'static str, call: fn(&[u8]) -> T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self {
            refl_type: TypeId::of::<T>(),
            refl_name: type_name::<T>(),
            kind: AssetsLoader::Glob {
                path_base,
                path_glob,
                func: AssetsGlobFn(Box::new(move |data| Arc::new(call(data)))),
                cached: HashMap::new(),
            },
        }
    }

    pub fn glob_defer(
        path_base: &'static str,
        path_glob: &'static str,
        func: fn(&[u8]) -> Vec<u8>,
    ) -> Self {
        Self {
            refl_type: TypeId::of::<Utf8PathBuf>(),
            refl_name: type_name::<Utf8PathBuf>(),
            kind: AssetsLoader::GlobDefer {
                path_base,
                path_glob,
                cached: HashMap::new(),
                bookkeeping: Arc::new(BookkeepingDeferred::new(func)),
            },
        }
    }

    pub fn glob_style(path_base: &'static str, path_glob: &'static str) -> Self {
        Self {
            refl_type: TypeId::of::<Stylesheet>(),
            refl_name: type_name::<Stylesheet>(),
            kind: AssetsLoader::GlobStyles {
                path_base,
                path_glob,
                cached: HashMap::new(),
            },
        }
    }

    pub fn glob_scripts(path_base: &'static str, path_glob: &'static str) -> Self {
        Self {
            refl_type: TypeId::of::<Script>(),
            refl_name: type_name::<Script>(),
            kind: AssetsLoader::GlobScripts {
                path_base,
                path_glob,
                cached: HashMap::new(),
            },
        }
    }

    #[cfg(feature = "images")]
    pub fn glob_images(path_base: &'static str, path_glob: &'static str) -> Self {
        Self {
            refl_type: TypeId::of::<()>(),
            refl_name: type_name::<()>(),
            kind: AssetsLoader::DeferredTwoStage {
                path_base,
                path_glob,
                cached: HashMap::new(),
            },
        }
    }

    pub fn glob_svelte(path_base: &'static str, path_glob: &'static str) -> Self {
        Self {
            refl_type: TypeId::of::<Svelte>(),
            refl_name: type_name::<Svelte>(),
            kind: AssetsLoader::GlobSvelte {
                path_base,
                path_glob,
                cached: HashMap::new(),
            },
        }
    }

    /// Load all assets which are matched by the defined glob.
    pub(crate) fn load(&mut self) {
        match &mut self.kind {
            AssetsLoader::Glob {
                path_base,
                path_glob,
                func: AssetsGlobFn(func),
                cached,
            } => {
                let pattern = Utf8Path::new(path_base).join(path_glob);
                let iter = glob::glob(pattern.as_str()).unwrap();

                for entry in iter {
                    let entry = Utf8PathBuf::try_from(entry.unwrap()).unwrap();
                    let bytes = fs::read(&entry).unwrap();

                    let hash = Hash32::hash(&bytes);
                    let data = func(&bytes);

                    cached.insert(
                        entry.to_owned(),
                        InputItem {
                            refl_type: self.refl_type,
                            refl_name: self.refl_name,
                            hash,
                            file: entry.to_owned(),
                            slug: entry.strip_prefix(&path_base).unwrap_or(&entry).to_owned(),
                            data: Input::Just(data),
                            info: None,
                        },
                    );
                }
            }
            AssetsLoader::GlobDefer {
                path_base,
                path_glob,
                cached,
                bookkeeping,
            } => {
                let pattern = Utf8Path::new(path_base).join(path_glob);
                let iter = glob::glob(pattern.as_str()).unwrap();

                cached.clear();
                for entry in iter {
                    let entry = Utf8PathBuf::try_from(entry.unwrap()).unwrap();
                    let bytes = fs::read(&entry).unwrap();

                    let hash = Hash32::hash(&bytes);

                    cached.insert(
                        entry.to_owned(),
                        InputItem {
                            refl_type: self.refl_type,
                            refl_name: self.refl_name,
                            hash,
                            file: entry.to_owned(),
                            slug: entry.strip_prefix(&path_base).unwrap_or(&entry).to_owned(),
                            data: {
                                let bookkeeping = bookkeeping.clone();

                                Input::Lazy(LazyLock::new(Box::new(move || {
                                    let path = build_deferred(hash, &entry, bookkeeping).unwrap();
                                    Arc::new(path)
                                })))
                            },
                            info: None,
                        },
                    );
                }
            }
            AssetsLoader::GlobStyles {
                path_base,
                path_glob,
                cached,
            } => {
                let pattern = Utf8Path::new(path_base).join(path_glob);
                let iter = glob::glob(pattern.as_str()).unwrap();

                for entry in iter {
                    let entry = Utf8PathBuf::try_from(entry.unwrap()).unwrap();

                    let opts = grass::Options::default().style(grass::OutputStyle::Compressed);
                    let stylesheet = grass::from_path(&entry, &opts).unwrap();

                    cached.insert(
                        entry.to_owned(),
                        InputItem {
                            refl_type: self.refl_type,
                            refl_name: self.refl_name,
                            hash: Hash32::hash(&stylesheet),
                            file: entry.to_owned(),
                            slug: entry.strip_prefix(&path_base).unwrap_or(&entry).to_owned(),
                            data: Input::Lazy(LazyLock::new(Box::new(move || {
                                let hash = Hash32::hash(&stylesheet);
                                let path =
                                    write_hashed_data(stylesheet.as_bytes(), hash, "css").unwrap();

                                Arc::new(Stylesheet { path })
                            }))),
                            info: None,
                        },
                    );
                }
            }
            AssetsLoader::GlobScripts {
                path_base,
                path_glob,
                cached,
            } => {
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
                    let result = compile_esbuild(&file_path);
                    let result_hash = Hash32::hash(&result);

                    cached.insert(
                        file_path.to_owned(),
                        InputItem {
                            refl_type: self.refl_type,
                            refl_name: self.refl_name,
                            slug: file_path.clone(),
                            file: file_path.clone(),
                            hash: result_hash,
                            data: Input::Lazy(LazyLock::new(Box::new(move || {
                                let hash = Hash32::hash(&result);
                                let path = write_hashed_data(&result, hash, "js").unwrap();

                                Arc::new(Script { path })
                            }))),
                            info: None,
                        },
                    );
                }
            }
            #[cfg(feature = "images")]
            AssetsLoader::DeferredTwoStage {
                path_base,
                path_glob,
                cached,
            } => {
                let pattern = Utf8Path::new(path_base).join(path_glob);
                let iter = glob::glob(pattern.as_str()).unwrap();

                for entry in iter {
                    let entry = Utf8PathBuf::try_from(entry.unwrap()).unwrap();
                    let bytes = fs::read(&entry).unwrap();
                    let hash = Hash32::hash(&bytes);

                    cached.insert(
                        entry.to_owned(),
                        InputItem {
                            refl_type: self.refl_type,
                            refl_name: self.refl_name,
                            hash,
                            file: entry.to_owned(),
                            slug: entry.strip_prefix(&path_base).unwrap_or(&entry).to_owned(),
                            data: Input::Lazy(LazyLock::new(Box::new(move || {
                                use crate::{Image, runtime::build_image};

                                let path = build_image(hash, &entry).unwrap();
                                Arc::new(Image { path })
                            }))),
                            info: None,
                        },
                    );
                }
            }
            AssetsLoader::GlobSvelte {
                path_base,
                path_glob,
                cached,
            } => {
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
                    let hash_class = Hash32::hash(file_path.as_str());
                    let html = compile_svelte_html(&file_path, hash_class);
                    let init = compile_svelte_init(&file_path, hash_class);

                    let mut hasher = Sha256::new();
                    hasher.update(&html);
                    hasher.update(&init);
                    let result_hash: Hash32 = hasher.finalize().into();

                    cached.insert(
                        file_path.to_owned(),
                        InputItem {
                            refl_type: self.refl_type,
                            refl_name: self.refl_name,
                            slug: file_path.clone(),
                            file: file_path.clone(),
                            hash: result_hash,
                            data: Input::Lazy(LazyLock::new(Box::new(move || {
                                let hash = Hash32::hash(&init);
                                let init = write_hashed_data(init.as_bytes(), hash, "js").unwrap();
                                let html = html.to_owned();

                                Arc::new(Svelte { html, init })
                            }))),
                            info: None,
                        },
                    );
                }
            }
        }
    }

    pub(crate) fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> bool {
        match &mut self.kind {
            AssetsLoader::Glob {
                path_base,
                path_glob,
                func: AssetsGlobFn(func),
                cached,
            } => {
                let pattern = Utf8Path::new(path_base).join(path_glob);
                let matcher = glob::Pattern::new(pattern.as_str()).unwrap();
                let mut changed = false;

                for entry in set {
                    if !matcher.matches_path(entry.as_std_path()) {
                        continue;
                    }

                    let bytes = fs::read(entry).unwrap();
                    let hash = Hash32::hash(&bytes);
                    let data = func(&bytes);

                    cached.insert(
                        entry.to_owned(),
                        InputItem {
                            refl_type: self.refl_type,
                            refl_name: self.refl_name,
                            hash,
                            file: entry.to_owned(),
                            slug: entry.strip_prefix(&path_base).unwrap_or(entry).to_owned(),
                            data: Input::Just(data),
                            info: None,
                        },
                    );
                    changed = true;
                }

                changed
            }
            AssetsLoader::GlobDefer {
                path_base,
                path_glob,
                cached,
                bookkeeping,
            } => {
                let pattern = Utf8Path::new(path_base).join(path_glob);
                let matcher = glob::Pattern::new(pattern.as_str()).unwrap();
                let mut changed = false;

                for entry in set {
                    if !matcher.matches_path(entry.as_std_path()) {
                        continue;
                    }

                    let bytes = fs::read(entry).unwrap();
                    let hash = Hash32::hash(&bytes);

                    cached.insert(
                        entry.to_owned(),
                        InputItem {
                            refl_type: self.refl_type,
                            refl_name: self.refl_name,
                            hash,
                            file: entry.to_owned(),
                            slug: entry.strip_prefix(&path_base).unwrap_or(&entry).to_owned(),
                            data: {
                                let entry = entry.to_owned();
                                let bookkeeping = bookkeeping.clone();

                                Input::Lazy(LazyLock::new(Box::new(move || {
                                    let path = build_deferred(hash, &entry, bookkeeping).unwrap();
                                    Arc::new(path)
                                })))
                            },
                            info: None,
                        },
                    );
                    changed = true;
                }

                changed
            }
            AssetsLoader::GlobStyles { path_base, .. } => {
                if set.iter().any(|path| path.starts_with(&path_base)) {
                    self.load();
                    true
                } else {
                    false
                }
            }
            AssetsLoader::GlobScripts { path_base, .. } => {
                if set.iter().any(|path| path.starts_with(&path_base)) {
                    self.load();
                    true
                } else {
                    false
                }
            }
            #[cfg(feature = "images")]
            AssetsLoader::DeferredTwoStage {
                path_base,
                path_glob,
                cached,
            } => {
                let pattern = Utf8Path::new(path_base).join(path_glob);
                let matcher = glob::Pattern::new(pattern.as_str()).unwrap();
                let mut changed = false;

                for entry in set {
                    if !matcher.matches_path(entry.as_std_path()) {
                        continue;
                    }

                    let bytes = fs::read(entry).unwrap();
                    let hash = Hash32::hash(&bytes);

                    cached.insert(
                        entry.to_owned(),
                        InputItem {
                            refl_type: self.refl_type,
                            refl_name: self.refl_name,
                            hash,
                            file: entry.to_owned(),
                            slug: entry.strip_prefix(&path_base).unwrap_or(entry).to_owned(),
                            data: {
                                use crate::{Image, runtime::build_image};

                                let entry = entry.clone();
                                Input::Lazy(LazyLock::new(Box::new(move || {
                                    let path = build_image(hash, &entry).unwrap();
                                    Arc::new(Image { path })
                                })))
                            },
                            info: None,
                        },
                    );
                    changed = true;
                }

                changed
            }
            AssetsLoader::GlobSvelte { path_base, .. } => {
                if set.iter().any(|path| path.starts_with(&path_base)) {
                    self.load();
                    true
                } else {
                    false
                }
            }
        }
    }

    pub(crate) fn items(&self) -> Vec<&InputItem> {
        match &self.kind {
            AssetsLoader::Glob { cached, .. }
            | AssetsLoader::GlobDefer { cached, .. }
            | AssetsLoader::GlobStyles { cached, .. }
            | AssetsLoader::GlobScripts { cached, .. }
            | AssetsLoader::GlobSvelte { cached, .. } => cached.values().collect(),
            #[cfg(feature = "images")]
            AssetsLoader::DeferredTwoStage { cached, .. } => cached.values().collect(),
        }
    }

    pub(crate) fn path_base(&self) -> &'static str {
        match &self.kind {
            AssetsLoader::Glob { path_base, .. }
            | AssetsLoader::GlobDefer { path_base, .. }
            | AssetsLoader::GlobStyles { path_base, .. }
            | AssetsLoader::GlobScripts { path_base, .. }
            | AssetsLoader::GlobSvelte { path_base, .. } => path_base,
            #[cfg(feature = "images")]
            AssetsLoader::DeferredTwoStage { path_base, .. } => path_base,
        }
    }

    pub(crate) fn remove(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool {
        match &mut self.kind {
            AssetsLoader::Glob { cached, .. }
            | AssetsLoader::GlobDefer { cached, .. }
            | AssetsLoader::GlobStyles { cached, .. }
            | AssetsLoader::GlobScripts { cached, .. }
            | AssetsLoader::GlobSvelte { cached, .. } => {
                let before = cached.len();
                cached.retain(|path, _| !obsolete.contains(path));
                cached.len() < before
            }
            #[cfg(feature = "images")]
            AssetsLoader::DeferredTwoStage { cached, .. } => {
                let before = cached.len();
                cached.retain(|path, _| !obsolete.contains(path));
                cached.len() < before
            }
        }
    }
}
