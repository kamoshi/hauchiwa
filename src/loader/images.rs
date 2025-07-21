use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    BuildError, Hash32,
    loader::{Loader, generic::LoaderGeneric},
};

/// Represents a hashed, losslessly compressed WebP image ready for use in templates.
///
/// The `path` points to the output location under the `/hash/img/` virtual namespace,
/// suitable for use in `src` attributes or manifest generation. Images are
/// content-addressed, allowing caching, deduplication, and incremental builds.
///
/// Created by `glob_images` from any raster input (e.g., PNG, JPEG, etc.).
pub struct Image {
    /// Relative path to the optimized WebP image, rooted at `/hash/img/`.
    pub path: Utf8PathBuf,
}

/// Constructs a loader that ingests raster images and emits lossless WebP variants.
///
/// Matches files using a glob pattern relative to `path_base`, reads each file,
/// and re-encodes it as WebP using the [`image`] crate's lossless encoder. The
/// resulting output is hashed and cached, then emitted into `dist`. The final
/// [`Image`] contains a path pointing to the output asset.
///
/// ### Parameters
/// - `path_base`: Base directory for relative glob resolution.
/// - `path_glob`: Glob pattern matching source image files (e.g. `"**/*.png"`).
///
/// ### Returns
/// A [`Loader`] that emits [`Image`] objects keyed by file content.
///
/// ### Example
/// ```rust
/// use hauchiwa::loader::glob_images;
///
/// let loader = glob_images("assets/images", "**/*.png");
/// ```
///
/// ### Notes
/// - Only lossless WebP encoding is currently supported.
/// - All outputs are content-addressed: the same input will always yield
///   the same output path.
/// - Image decoding/encoding is synchronous; performance may vary with size and volume.
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
