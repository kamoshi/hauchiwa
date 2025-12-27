use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

use crate::error::{BuildError, HauchiwaError};
use crate::loader::{Assets, GlobAssetsTask, Input};
use crate::{Blueprint, Handle};

/// Errors that can occur when processing images.
#[derive(Debug, Error)]
pub enum ImageError {
    /// An I/O error occurred while reading or writing image files.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// An error occurred during image decoding or encoding.
    #[error("Image processing error: {0}")]
    Image(#[from] image::ImageError),

    /// An internal build error.
    #[error("Build error: {0}")]
    Build(#[from] BuildError),
}

/// Represents a processed image asset.
///
/// Images loaded via `SiteConfig::glob_images` are automatically optimized
/// and cached. This struct provides the path to the optimized version.
#[derive(Clone)]
pub struct Image {
    /// The web-accessible path to the optimized image (e.g., `/hash/img/abc1234.webp`).
    pub path: Utf8PathBuf,
}

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Registers an image loader that optimizes and caches images.
    ///
    /// This loader finds images matching the provided glob patterns, converts
    /// them to generic WebP format, and stores them in the distribution
    /// directory. It uses content hashing to avoid re-processing images that
    /// haven't changed.
    ///
    /// # Arguments
    ///
    /// * `path_glob` - A slice of glob patterns to find images (e.g., `&["assets/**/*.png", "photos/*.jpg"]`).
    ///
    /// # Returns
    ///
    /// A [`Handle`] to a [`Assets<Image>`], mapping original file paths to the processed [`Image`] struct.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # let mut config = hauchiwa::Blueprint::<()>::new();
    /// // Load all PNG and JPG images in the assets directory.
    /// let images = config.load_images(&["assets/**/*.png", "assets/**/*.jpg"]);
    /// ```
    pub fn load_images(
        &mut self,
        path_glob: &'static [&'static str],
    ) -> Result<Handle<Assets<Image>>, HauchiwaError> {
        Ok(self.add_task_opaque(GlobAssetsTask::new(
            path_glob.to_vec(),
            path_glob.to_vec(),
            move |_, _, input: Input| {
                let path = build_image(&input)?;

                Ok((input.path, Image { path }))
            },
        )?))
    }
}

fn process_image(buffer: &[u8]) -> Result<Vec<u8>, ImageError> {
    let img = image::load_from_memory(buffer)?;
    let w = img.width();
    let h = img.height();

    let mut out = Vec::new();
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);

    encoder.encode(&img.to_rgba8(), w, h, image::ExtendedColorType::Rgba8)?;

    Ok(out)
}

fn build_image(file: &Input) -> Result<Utf8PathBuf, ImageError> {
    let hash = file.hash.to_hex();
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
        let buffer = file.read()?;
        let buffer = process_image(&buffer)?;

        fs::create_dir_all(".cache/hash/img/")?;
        fs::write(&path_hash, buffer)?;
    }

    let dir = path_dist.parent().unwrap_or(&path_dist);
    fs::create_dir_all(dir)?;
    fs::copy(&path_hash, &path_dist)?;

    Ok(path_root)
}
