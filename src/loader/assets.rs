use std::fmt::Debug;
use std::fs;
use std::process::{Command, Stdio};
use std::sync::RwLock;
use std::time::Instant;
use std::{collections::HashMap, sync::Arc};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{ArcAny, Hash32, Input, InputItem, InputStylesheet};

type BoxFn8 = Box<dyn Fn(&[u8]) -> ArcAny + Send + Sync>;

struct AssetsGlobFn(BoxFn8);

impl Debug for AssetsGlobFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fn(*)")
    }
}

#[derive(Debug)]
pub(crate) struct Bookkeeping {
    pub(crate) func: fn(&[u8]) -> Vec<u8>,
    pub(crate) done: RwLock<HashMap<Hash32, Hash32>>,
}

impl Bookkeeping {
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

#[derive(Debug)]
pub struct Assets(AssetsLoader);

#[derive(Debug)]
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
        bookkeeping: Arc<Bookkeeping>,
    },
    GlobStyle {
        path_base: &'static str,
        path_glob: &'static str,
        cached: Vec<InputItem>,
    },
    GlobScripts {
        path_base: &'static str,
        path_glob: &'static str,
        cached: Vec<InputItem>,
    },
}

impl Assets {
    pub fn glob<T>(path_base: &'static str, path_glob: &'static str, call: fn(&[u8]) -> T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self(AssetsLoader::Glob {
            path_base,
            path_glob,
            func: AssetsGlobFn(Box::new(move |data| Arc::new(call(data)))),
            cached: HashMap::new(),
        })
    }

    pub fn glob_defer(
        path_base: &'static str,
        path_glob: &'static str,
        func: fn(&[u8]) -> Vec<u8>,
    ) -> Self {
        Self(AssetsLoader::GlobDefer {
            path_base,
            path_glob,
            cached: HashMap::new(),
            bookkeeping: Arc::new(Bookkeeping::new(func)),
        })
    }

    pub fn glob_style(path_base: &'static str, path_glob: &'static str) -> Self {
        Self(AssetsLoader::GlobStyle {
            path_base,
            path_glob,
            cached: Vec::new(),
        })
    }

    pub fn glob_scripts(path_base: &'static str, path_glob: &'static str) -> Self {
        Self(AssetsLoader::GlobScripts {
            path_base,
            path_glob,
            cached: Vec::new(),
        })
    }

    /// Load all assets which are matched by the defined glob.
    pub(crate) fn load(&mut self) {
        match &mut self.0 {
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
                            hash,
                            file: entry.to_owned(),
                            slug: entry.strip_prefix(&path_base).unwrap_or(&entry).to_owned(),
                            data: Input::InMemory(data),
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
                            hash,
                            file: entry.to_owned(),
                            slug: entry.strip_prefix(&path_base).unwrap_or(&entry).to_owned(),
                            data: Input::OnDisk(bookkeeping.clone()),
                            info: None,
                        },
                    );
                }
            }
            AssetsLoader::GlobStyle {
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

                    cached.push(InputItem {
                        hash: Hash32::hash(&stylesheet),
                        file: entry.to_owned(),
                        slug: entry.strip_prefix(&path_base).unwrap_or(&entry).to_owned(),
                        data: Input::Stylesheet(InputStylesheet { stylesheet }),
                        info: None,
                    });
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
                    let output = Command::new("esbuild")
                        .arg(file_path.as_str())
                        .arg("--format=esm")
                        .arg("--bundle")
                        .arg("--minify")
                        .stdout(Stdio::piped())
                        .stderr(Stdio::inherit())
                        .output()
                        .expect("esbuild invocation failed");

                    let result = output.stdout;
                    let result_hash = Hash32::hash(&result);
                    let result_hash_hex = result_hash.to_hex();

                    let path_dist = Utf8Path::new(".cache/hash").join(&result_hash_hex);

                    let dir = path_dist.parent().unwrap_or(&path_dist);
                    fs::create_dir_all(dir).unwrap();
                    fs::write(&path_dist, result).unwrap();

                    cached.push(InputItem {
                        slug: file_path.clone(),
                        file: file_path.clone(),
                        hash: result_hash,
                        data: Input::Script,
                        info: None,
                    });
                }
            }
        }
    }

    pub(crate) fn items(&self) -> Vec<&InputItem> {
        match &self.0 {
            AssetsLoader::Glob { cached, .. } | AssetsLoader::GlobDefer { cached, .. } => {
                cached.values().collect()
            }
            AssetsLoader::GlobStyle { cached, .. } | AssetsLoader::GlobScripts { cached, .. } => {
                cached.iter().collect()
            }
        }
    }
}
