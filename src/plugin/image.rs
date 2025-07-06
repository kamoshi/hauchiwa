use std::{
    any::{TypeId, type_name},
    collections::{HashMap, HashSet},
    fs,
    sync::{Arc, LazyLock},
};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{BuilderError, Hash32, HauchiwaError, Input, InputItem};

pub struct Image {
    pub path: Utf8PathBuf,
}

pub struct LoaderImage {
    path_base: &'static str,
    path_glob: &'static str,
    cached: HashMap<Utf8PathBuf, InputItem>,
}

impl LoaderImage {
    pub fn new(path_base: &'static str, path_glob: &'static str) -> Self {
        Self {
            path_base,
            path_glob,
            cached: HashMap::new(),
        }
    }
}

impl super::Loadable for LoaderImage {
    fn load(&mut self) {
        let Self {
            path_base,
            path_glob,
            cached,
        } = self;

        let pattern = Utf8Path::new(path_base).join(path_glob);
        let iter = glob::glob(pattern.as_str()).unwrap();

        for entry in iter {
            let entry = Utf8PathBuf::try_from(entry.unwrap()).unwrap();
            let bytes = fs::read(&entry).unwrap();
            let hash = Hash32::hash(&bytes);

            cached.insert(
                entry.to_owned(),
                InputItem {
                    refl_type: TypeId::of::<Image>(),
                    refl_name: type_name::<Image>(),
                    hash,
                    file: entry.to_owned(),
                    slug: entry.strip_prefix(&path_base).unwrap_or(&entry).to_owned(),
                    data: Input::Lazy(LazyLock::new(Box::new(move || {
                        let path = build_image(hash, &entry).unwrap();
                        Arc::new(Image { path })
                    }))),
                    info: None,
                },
            );
        }
    }

    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> bool {
        let Self {
            path_base,
            path_glob,
            cached,
        } = self;

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
                    refl_type: TypeId::of::<Image>(),
                    refl_name: type_name::<Image>(),
                    hash,
                    file: entry.to_owned(),
                    slug: entry.strip_prefix(&path_base).unwrap_or(entry).to_owned(),
                    data: {
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

    fn items(&self) -> Vec<&InputItem> {
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

fn process_image(buffer: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(buffer).expect("Couldn't load image");
    let w = img.width();
    let h = img.height();

    let mut out = Vec::new();
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);

    encoder
        .encode(&img.to_rgba8(), w, h, image::ExtendedColorType::Rgba8)
        .expect("Encoding error");

    out
}

fn build_image(hash: Hash32, file: &Utf8Path) -> Result<Utf8PathBuf, HauchiwaError> {
    let hash = hash.to_hex();
    let path_root = Utf8Path::new("/hash/img/")
        .join(&hash)
        .with_extension("webp");
    let path_hash = Utf8Path::new(".cache/hash/img/")
        .join(&hash)
        .with_extension("webp");
    let path_dist = Utf8Path::new("dist/hash/img/")
        .join(&hash)
        .with_extension("webp");

    // If this hash exists it means the work is already done.
    if !path_hash.exists() {
        let buffer = fs::read(file) //
            .map_err(|e| BuilderError::FileReadError(file.to_path_buf(), e))?;
        let buffer = process_image(&buffer);

        fs::create_dir_all(".cache/hash/img/")
            .map_err(|e| BuilderError::CreateDirError(".cache/hash".into(), e))?;
        fs::write(&path_hash, buffer).unwrap();
    }

    let dir = path_dist.parent().unwrap_or(&path_dist);
    fs::create_dir_all(dir) //
        .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
    fs::copy(&path_hash, &path_dist)
        .map_err(|e| BuilderError::FileCopyError(path_hash.to_owned(), path_dist.clone(), e))?;

    Ok(path_root)
}
