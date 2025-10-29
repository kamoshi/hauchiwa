use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    error::BuildError,
    loader::{File, FileLoaderTask},
    task::Handle,
    Hash32, SiteConfig,
};

/// Represents a hashed, losslessly compressed WebP image ready for use in templates.
///
/// The `path` points to the output location under the `/hash/img/` virtual namespace,
/// suitable for use in `src` attributes or manifest generation. Images are
/// content-addressed, allowing caching, deduplication, and incremental builds.
///
/// Created by `glob_images` from any raster input (e.g., PNG, JPEG, etc.).
#[derive(Clone)]
pub struct Image {
    /// Relative path to the optimized WebP image, rooted at `/hash/img/`.
    pub path: Utf8PathBuf,
}

pub fn glob_images(
    site_config: &mut SiteConfig,
    path_base: &'static str,
    path_glob: &'static str,
) -> Handle<Vec<Image>> {
    let task = FileLoaderTask::new(path_base, path_glob, move |file| {
        let path = build_image(&file.metadata)?;
        Ok(Image { path })
    });
    site_config.add_task_boxed(Box::new(task))
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

fn build_image(buffer: &[u8]) -> Result<Utf8PathBuf, BuildError> {
    let hash = Hash32::hash(buffer);
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
        let buffer = process_image(buffer) //
            .map_err(|err| BuildError::Other(err.into()))?;

        fs::create_dir_all(".cache/hash/img/")?;
        fs::write(&path_hash, buffer)?;
    }

    let dir = path_dist.parent().unwrap_or(&path_dist);
    fs::create_dir_all(dir)?;
    fs::copy(&path_hash, &path_dist)?;

    Ok(path_root)
}
