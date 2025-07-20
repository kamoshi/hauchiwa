use camino::Utf8PathBuf;
use grass::{Options, OutputStyle, from_path};

use crate::{Hash32, Loader, loader::generic::LoaderGenericMultifile};

/// Represents a compiled CSS asset emitted by the build pipeline.
///
/// This struct contains only the path to the minified stylesheet,
/// which can be included in HTML templates or referenced from other assets.
pub struct Style {
    /// Path to the generated CSS file.
    pub path: Utf8PathBuf,
}

/// Constructs a loader that compiles all `.scss` files matching the given glob pattern.
///
/// Uses [`grass`] (a Sass compiler in Rust) to compile matched files with compressed
/// output style. Each compiled result is hashed for content-based caching and stored
/// as a `.css` file. The resulting [`Style`] contains a path to the compiled stylesheet.
///
/// ### Parameters
/// - `path_base`: Base directory used for resolving relative paths.
/// - `path_glob`: Glob pattern to select `.scss` files within `path_base`.
///
/// ### Returns
/// A [`Loader`] that emits [`Style`] objects keyed by file path and content hash.
///
/// ### Example
/// ```rust
/// use hauchiwa::loader::glob_styles;
///
/// let loader = glob_styles("src/styles", "**/*.scss");
/// ```
pub fn glob_styles(path_base: &'static str, path_glob: &'static str) -> Loader {
    Loader::with(move |_| {
        LoaderGenericMultifile::new(
            path_base,
            path_glob,
            |path| {
                let opts = Options::default().style(OutputStyle::Compressed);
                let data = from_path(path, &opts)?;
                let hash = Hash32::hash(&data);

                Ok((hash, data))
            },
            |rt, data| {
                let path = rt.store(data.as_bytes(), "css")?;

                Ok(Style { path })
            },
        )
    })
}
