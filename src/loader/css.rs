use thiserror::Error;

use crate::Hash32;
use crate::engine::HandleF;
use crate::loader::GlobBundle;
use crate::{Blueprint, error::HauchiwaError};

/// Errors that can occur when compiling Stylesheets.
#[derive(Debug, Error)]
pub enum StyleError {
    /// An I/O error occurred.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A Sass compilation error occurred.
    #[error("Sass compilation error: {0}")]
    Sass(#[from] Box<grass::Error>),

    /// An internal build error.
    #[error("Build error: {0}")]
    Build(#[from] crate::error::BuildError),
}

/// Represents a compiled CSS file.
#[derive(Debug, Clone)]
pub struct Stylesheet {
    /// The path to the compiled CSS file.
    pub path: camino::Utf8PathBuf,
}

/// A builder for configuring the CSS loader task.
pub struct CssLoader<'a, G>
where
    G: Send + Sync,
{
    blueprint: &'a mut Blueprint<G>,
    entry_globs: Vec<String>,
    watch_globs: Vec<String>,
    minify: bool,
}

impl<'a, G> CssLoader<'a, G>
where
    G: Send + Sync + 'static,
{
    fn new(blueprint: &'a mut Blueprint<G>) -> Self {
        Self {
            blueprint,
            entry_globs: Vec::new(),
            watch_globs: Vec::new(),
            minify: true,
        }
    }

    /// Adds a glob pattern to find entry stylesheets (e.g., "styles/main.scss").
    pub fn entry(mut self, glob: impl Into<String>) -> Self {
        self.entry_globs.push(glob.into());
        self
    }

    /// Adds a glob pattern for files to watch (e.g., "styles/**/*.scss").
    ///
    /// If never called, defaults to watching the entry globs.
    pub fn watch(mut self, glob: impl Into<String>) -> Self {
        self.watch_globs.push(glob.into());
        self
    }

    /// Configures minification (compression). Defaults to `true`.
    pub fn minify(mut self, minify: bool) -> Self {
        self.minify = minify;
        self
    }

    /// Registers the task with the Blueprint.
    pub fn register(self) -> Result<HandleF<Stylesheet>, HauchiwaError> {
        let watch_globs = if self.watch_globs.is_empty() {
            self.entry_globs.clone()
        } else {
            self.watch_globs
        };

        let minify = self.minify;

        let task = GlobBundle::new(self.entry_globs, watch_globs, move |_, store, input| {
            let style = if minify {
                grass::OutputStyle::Compressed
            } else {
                grass::OutputStyle::Expanded
            };

            let options = grass::Options::default().style(style);

            let data = grass::from_path(&input.path, &options).map_err(StyleError::Sass)?;
            let hash = Hash32::hash(&data);

            let path = store
                .save(data.as_bytes(), "css")
                .map_err(StyleError::Build)?;

            Ok((hash, input.path, Stylesheet { path }))
        })?;

        Ok(self.blueprint.add_task_fine(task))
    }
}

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Starts configuring a CSS loader task.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # let mut config = hauchiwa::Blueprint::<()>::new();
    /// config.load_css()
    ///     .entry("styles/main.scss")
    ///     .watch("styles/**/*.scss")
    ///     .minify(true)
    ///     .register()?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn load_css(&mut self) -> CssLoader<'_, G> {
        CssLoader::new(self)
    }
}
