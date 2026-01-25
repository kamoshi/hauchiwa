use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};

use camino::{Utf8Path, Utf8PathBuf};
use image::{ExtendedColorType, ImageReader};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::{BuildError, HauchiwaError};
use crate::loader::{Assets, GlobAssetsTask, Input, Store};
use crate::{Blueprint, Handle, TaskContext};

const DIR_STORE: &str = "/hash/img/";
const DIR_CACHE: &str = ".cache/hash/img/";
const DIR_DIST: &str = "dist/hash/img/";

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

/// Configuration for image compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quality {
    /// Lossless compression.
    Lossless,
    /// Lossy compression with a quality factor (0-100).
    Lossy(u8),
}

impl Default for Quality {
    fn default() -> Self {
        // A sensible default for most web images
        Self::Lossy(80)
    }
}

/// Supported output image formats with specific configuration.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    #[default]
    WebP,
    Avif(Quality),
    Png,
}

impl ImageFormat {
    fn extension(&self) -> &'static str {
        match self {
            ImageFormat::WebP => "webp",
            ImageFormat::Avif(_) => "avif",
            ImageFormat::Png => "png",
        }
    }
}

/// Represents a processed image asset with multiple formats.
#[derive(Clone, Debug)]
pub struct Image {
    /// The default image path (usually the first configured format).
    pub default: Utf8PathBuf,
    /// A map of available formats to their web-accessible paths.
    pub sources: HashMap<ImageFormat, Utf8PathBuf>,
    /// The original width of the image.
    pub width: u32,
    /// The original height of the image.
    pub height: u32,
}

impl Image {
    /// Helper to get the path for a specific format.
    pub fn get(&self, format: ImageFormat) -> Option<&Utf8PathBuf> {
        self.sources.get(&format)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ImageMetadata {
    width: u32,
    height: u32,
}

/// A builder for configuring the image loading task.
pub struct ImageLoader<'a, G>
where
    G: Send + Sync,
{
    blueprint: &'a mut Blueprint<G>,
    globs: Vec<&'static str>,
    formats: Vec<ImageFormat>,
}

impl<'a, G> ImageLoader<'a, G>
where
    G: Send + Sync + 'static,
{
    fn new(blueprint: &'a mut Blueprint<G>) -> Self {
        Self {
            blueprint,
            globs: Vec::new(),
            formats: Vec::new(),
        }
    }

    /// Adds a glob pattern to find images.
    pub fn source(mut self, glob: &'static str) -> Self {
        self.globs.push(glob);
        self
    }

    /// Adds an output format to generate.
    ///
    /// The first format added will be considered the "default" for the `Image` struct.
    pub fn format(mut self, format: ImageFormat) -> Self {
        if !self.formats.contains(&format) {
            self.formats.push(format);
        }
        self
    }

    /// Registers the task with the Blueprint.
    pub fn register(self) -> Result<Handle<Assets<Image>>, HauchiwaError> {
        let mut formats = self.formats;

        // Default to WebP if no format is specified
        if formats.is_empty() {
            formats.push(ImageFormat::default());
        }

        let task = GlobAssetsTask::new(
            self.globs.clone(),
            // watch the source globs
            self.globs,
            move |_: &TaskContext<G>, _: &mut Store, input: Input| {
                let image = process_image(&input, &formats)?;
                Ok((input.path, image))
            },
        )?;

        Ok(self.blueprint.add_task_opaque(task))
    }
}

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Starts configuring an image loader task.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # let mut config = hauchiwa::Blueprint::<()>::new();
    /// config.load_images()
    ///     .source("assets/images/*.jpg")
    ///     .format(hauchiwa::loader::image::ImageFormat::WebP)
    ///     .register()?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn load_images(&mut self) -> ImageLoader<'_, G> {
        ImageLoader::new(self)
    }
}

fn process_image(file: &Input, formats: &[ImageFormat]) -> Result<Image, ImageError> {
    let source_hash = file.hash.to_hex();

    let meta_file_name = format!("{}.meta.cbor", source_hash);
    let meta_file_path = Utf8Path::new(DIR_CACHE).join(&meta_file_name);

    fs::create_dir_all(DIR_CACHE)?;
    fs::create_dir_all(DIR_DIST)?;

    // Try to load serialized metadata
    let metadata = if meta_file_path.exists() {
        let file = File::open(&meta_file_path)?;
        let file = BufReader::new(file);

        ciborium::from_reader::<ImageMetadata, _>(file).ok()
    } else {
        None
    };

    // Calculate paths for all formats
    let mut outputs = Vec::new();
    let mut cached = true;

    for &format in formats {
        // Include configuration in the hash to ensure cache invalidation if quality changes
        let config = match format {
            ImageFormat::WebP => "webp".to_string(),
            ImageFormat::Avif(Quality::Lossy(q)) => format!("avif-q{}", q),
            ImageFormat::Avif(Quality::Lossless) => "avif-ll".to_string(),
            ImageFormat::Png => "png".to_string(),
        };

        // Final filename: <hash>.<config>.<ext>
        let file_name = format!("{}.{}.{}", source_hash, config, format.extension());

        let path_store = Utf8Path::new(DIR_STORE).join(&file_name);
        let path_cache = Utf8Path::new(DIR_CACHE).join(&file_name);
        let path_dist = Utf8Path::new(DIR_DIST).join(&file_name);

        if !path_cache.exists() {
            // cache miss
            cached = false;
        }

        outputs.push((format, path_store, path_cache, path_dist));
    }

    // FAST PATH: If metadata exists and all output formats are cached
    if cached && let Some(meta) = metadata {
        let mut sources = HashMap::new();
        let mut default_path = None;

        for (format, path_store, path_cache, path_dist) in outputs {
            // Ensure artifact is in dist
            if !path_dist.exists() {
                // hard link with fallback to copy
                if std::fs::hard_link(&path_cache, &path_dist).is_err() {
                    std::fs::copy(&path_cache, &path_dist)?;
                }
            }

            sources.insert(format, path_store.clone());

            if default_path.is_none() {
                default_path = Some(path_store);
            }
        }

        return Ok(Image {
            default: default_path.expect("At least one format must be produced"),
            sources,
            width: meta.width,
            height: meta.height,
        });
    }

    // SLOW PATH: Decode source image
    let reader = BufReader::new(File::open(&file.path)?);
    let img = ImageReader::new(reader).with_guessed_format()?.decode()?;
    let width = img.width();
    let height = img.height();
    let rgba = img.to_rgba8();

    // Save metadata
    let meta_data = ImageMetadata { width, height };
    let meta_file = File::create(&meta_file_path)?;
    ciborium::into_writer(&meta_data, meta_file).map_err(std::io::Error::other)?;

    let mut sources = HashMap::new();
    let mut default_path = None;

    for (format, path_store, path_cache, path_dist) in outputs {
        if !path_cache.exists() {
            let cache_file = File::create(&path_cache)?;
            let mut writer = BufWriter::new(cache_file);

            match format {
                ImageFormat::WebP => {
                    use image::codecs::webp::WebPEncoder;

                    WebPEncoder::new_lossless(&mut writer).encode(
                        &rgba,
                        width,
                        height,
                        ExtendedColorType::Rgba8,
                    )?;
                }
                ImageFormat::Avif(quality) => match quality {
                    Quality::Lossless => {
                        use image::ImageEncoder;
                        use image::codecs::avif::AvifEncoder;

                        AvifEncoder::new(&mut writer).write_image(
                            &rgba,
                            width,
                            height,
                            ExtendedColorType::Rgba8,
                        )?;
                    }
                    Quality::Lossy(q) => {
                        use image::ImageEncoder;
                        use image::codecs::avif::AvifEncoder;

                        AvifEncoder::new_with_speed_quality(&mut writer, 10, q).write_image(
                            &rgba,
                            width,
                            height,
                            ExtendedColorType::Rgba8,
                        )?;
                    }
                },
                ImageFormat::Png => {
                    use image::ImageEncoder;
                    use image::codecs::png::PngEncoder;

                    PngEncoder::new(&mut writer).write_image(
                        &rgba,
                        width,
                        height,
                        ExtendedColorType::Rgba8,
                    )?;
                }
            }
        }

        if !path_dist.exists() {
            // hard link with fallback to copy
            if std::fs::hard_link(&path_cache, &path_dist).is_err() {
                std::fs::copy(&path_cache, &path_dist)?;
            }
        }

        sources.insert(format, path_store.clone());

        if default_path.is_none() {
            default_path = Some(path_store);
        }
    }

    Ok(Image {
        default: default_path.expect("At least one format must be produced"),
        sources,
        width,
        height,
    })
}
