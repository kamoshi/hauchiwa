use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

use crate::{
    Hash32, SiteConfig,
    error::{BuildError, HauchiwaError},
    loader::{File, Registry, glob::GlobRegistryTask},
    task::Handle,
};

#[derive(Clone)]
pub struct Image {
    pub path: Utf8PathBuf,
}

pub fn glob_images<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path_glob: &'static [&'static str],
) -> Result<Handle<Registry<Image>>, HauchiwaError> {
    Ok(site_config.add_task_opaque(GlobRegistryTask::new(
        path_glob.to_vec(),
        path_glob.to_vec(),
        move |_, file: File<Vec<u8>>| {
            let hash = Hash32::hash_file(&file.path)?;
            let path = build_image(hash, &file.path)?;
            Ok((file.path, Image { path }))
        },
    )?))
}

fn process_image(buffer: &[u8]) -> image::ImageResult<Vec<u8>> {
    let img = image::load_from_memory(buffer)?;
    let w = img.width();
    let h = img.height();

    let mut out = Vec::new();
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);

    encoder.encode(&img.to_rgba8(), w, h, image::ExtendedColorType::Rgba8)?;

    Ok(out)
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
        let buffer = process_image(&buffer) //
            .map_err(|err| BuildError::Other(err.into()))?;

        fs::create_dir_all(".cache/hash/img/")?;
        fs::write(&path_hash, buffer)?;
    }

    let dir = path_dist.parent().unwrap_or(&path_dist);
    fs::create_dir_all(dir)?;
    fs::copy(&path_hash, &path_dist)?;

    Ok(path_root)
}
