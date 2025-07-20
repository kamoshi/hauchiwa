use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    BuildError, Hash32,
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

fn build_image(hash: Hash32, file: &Utf8Path) -> Result<Utf8PathBuf, BuildError> {
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
        let buffer = fs::read(file)?;
        let buffer = process_image(&buffer);

        fs::create_dir_all(".cache/hash/img/")?;
        fs::write(&path_hash, buffer)?;
    }

    let dir = path_dist.parent().unwrap_or(&path_dist);
    fs::create_dir_all(dir)?;
    fs::copy(&path_hash, &path_dist)?;

    Ok(path_root)
}
