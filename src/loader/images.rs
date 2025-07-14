use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    BuilderError, Hash32, HauchiwaError,
    loader::{Loader, generic::LoaderGeneric},
};

pub struct Image {
    pub path: Utf8PathBuf,
}

pub fn glob_images(path_base: &'static str, path_glob: &'static str) -> Loader {
    Loader::with(move |_| {
        LoaderGeneric::new(
            path_base,
            path_glob,
            |path| {
                let hash = Hash32::hash_file(path)?;

                Ok((hash, (hash, path.to_owned())))
            },
            |_, (hash, path)| {
                let path = build_image(hash, &path)?;
                Ok(Image { path })
            },
        )
    })
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
