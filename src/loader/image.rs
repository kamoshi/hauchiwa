use std::fs::{self, File};
use std::io::{BufReader, BufWriter};

use camino::{Utf8Path, Utf8PathBuf};
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageReader};
use thiserror::Error;

use crate::error::{BuildError, HauchiwaError};
use crate::loader::{Assets, GlobAssetsTask, Input};
use crate::{Blueprint, Handle};

const STORE: &str = "/hash/img/";
const CACHE: &str = ".cache/hash/img/";
const DIST: &str = "dist/hash/img/";

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
            vec![],
            move |_, _, input: Input| {
                let path = build_image(&input)?;

                Ok((input.path, Image { path }))
            },
        )?))
    }
}

fn build_image(file: &Input) -> Result<Utf8PathBuf, ImageError> {
    let hash = file.hash.to_hex();

    // Setup paths
    let path_store = Utf8Path::new(STORE).join(&hash).with_extension("webp");
    let path_cache = Utf8Path::new(CACHE).join(&hash).with_extension("webp");
    let path_dist = Utf8Path::new(DIST).join(&hash).with_extension("webp");

    if !path_cache.exists() {
        fs::create_dir_all(CACHE)?;

        let cache = File::create(&path_cache)?;
        let mut writer = BufWriter::new(cache);

        let source = File::open(&file.path)?;
        let reader = BufReader::new(source);

        process_image_to_writer(reader, &mut writer)?;
    }

    fs::create_dir_all(DIST)?;

    if std::fs::hard_link(&path_cache, &path_dist).is_err() {
        std::fs::copy(&path_cache, &path_dist)?;
    }

    Ok(path_store)
}

fn process_image_to_writer(
    reader: impl std::io::BufRead + std::io::Seek,
    writer: &mut impl std::io::Write,
) -> Result<(), ImageError> {
    let img = ImageReader::new(reader).with_guessed_format()?.decode()?;

    let w = img.width();
    let h = img.height();
    let rgba = img.into_rgba8();

    WebPEncoder::new_lossless(writer).encode(&rgba, w, h, ExtendedColorType::Rgba8)?;

    Ok(())
}
