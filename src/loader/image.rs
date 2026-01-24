use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};

use camino::{Utf8Path, Utf8PathBuf};
use image::{ExtendedColorType, ImageReader};
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

/// Supported output image formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    WebP,
    Avif,
    Png,
}

impl ImageFormat {
    fn extension(&self) -> &'static str {
        match self {
            ImageFormat::WebP => "webp",
            ImageFormat::Avif => "avif",
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
            formats.push(ImageFormat::WebP);
        }

        let task = GlobAssetsTask::new(
            self.globs.clone(),
            // watch the source globs
            self.globs,
            move |_ctx: &TaskContext<G>, _: &mut Store, input: Input| {
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
    ///     .watch("assets/images/**/*.jpg")
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

    // Decode the image once
    let reader = BufReader::new(File::open(&file.path)?);
    let img = ImageReader::new(reader).with_guessed_format()?.decode()?;
    let width = img.width();
    let height = img.height();
    let rgba = img.to_rgba8(); // Use to_rgba8 for consistency

    let mut sources = HashMap::new();
    let mut default_path = None;

    // Ensure directories exist
    fs::create_dir_all(DIR_CACHE)?;
    fs::create_dir_all(DIR_DIST)?;

    for &format in formats {
        let file_name = format!("{}.{}", source_hash, format.extension());

        let path_store = Utf8Path::new(DIR_STORE).join(&file_name);
        let path_cache = Utf8Path::new(DIR_CACHE).join(&file_name);
        let path_dist = Utf8Path::new(DIR_DIST).join(&file_name);

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
                ImageFormat::Avif => {
                    use image::{ImageEncoder, codecs::avif::AvifEncoder};
                    AvifEncoder::new(&mut writer).write_image(
                        &rgba,
                        width,
                        height,
                        ExtendedColorType::Rgba8,
                    )?;
                }
                ImageFormat::Png => {
                    use image::{ImageEncoder, codecs::png::PngEncoder};
                    PngEncoder::new(&mut writer).write_image(
                        &rgba,
                        width,
                        height,
                        ExtendedColorType::Rgba8,
                    )?;
                }
            }
        }

        if std::fs::hard_link(&path_cache, &path_dist).is_err() {
            std::fs::copy(&path_cache, &path_dist)?;
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
