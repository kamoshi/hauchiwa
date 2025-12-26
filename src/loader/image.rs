use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

use crate::{
    Hash32, SiteConfig,
    error::{BuildError, HauchiwaError},
    loader::{File, Registry, glob::GlobRegistryTask},
    task::Handle,
};

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
#[derive(Clone)]
pub struct Image {
    /// The path to the processed image file (e.g., in the distribution directory).
    pub path: Utf8PathBuf,
}

impl<G> SiteConfig<G>
where
    G: Send + Sync + 'static,
{
    /// Scans for image files matching the provided glob patterns and converts them to WebP.
    ///
    /// This loader processes images found via the glob patterns. It converts them to
    /// generic WebP format, hashes the content for caching, and places the result in the
    /// distribution directory.
    ///
    /// # Arguments
    ///
    /// * `path_glob`: A list of glob patterns to find images (e.g., `&["assets/images/**/*.png"]`).
    ///
    /// # Returns
    ///
    /// A handle to a registry mapping original file paths to `Image` objects.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let images = config.load_images(&["assets/**/*.png", "assets/**/*.jpg"])?;
    /// ```
    pub fn load_images(
        &mut self,
        path_glob: &'static [&'static str],
    ) -> Result<Handle<Registry<Image>>, HauchiwaError> {
        Ok(self.add_task_opaque(GlobRegistryTask::new(
            path_glob.to_vec(),
            path_glob.to_vec(),
            move |_, _, file: File<Vec<u8>>| {
                let hash = Hash32::hash_file(&file.path)?;
                let path = build_image(hash, &file.path)?;

                Ok((file.path, Image { path }))
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

fn build_image(hash: Hash32, file: &Utf8Path) -> Result<Utf8PathBuf, ImageError> {
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
        let buffer = process_image(&buffer)?;

        fs::create_dir_all(".cache/hash/img/")?;
        fs::write(&path_hash, buffer)?;
    }

    let dir = path_dist.parent().unwrap_or(&path_dist);
    fs::create_dir_all(dir)?;
    fs::copy(&path_hash, &path_dist)?;

    Ok(path_root)
}
