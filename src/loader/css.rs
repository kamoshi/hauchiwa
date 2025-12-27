use thiserror::Error;

use crate::{Blueprint, error::HauchiwaError, graph::Handle, loader::GlobAssetsTask};

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

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Compiles Sass/SCSS files to CSS.
    ///
    /// This loader uses the `grass` crate to compile Sass files matching the entry glob.
    /// It returns a registry of compiled CSS files.
    ///
    /// # Arguments
    ///
    /// * `glob_entry`: Glob pattern for the entry stylesheets (e.g., "styles/main.scss").
    /// * `glob_watch`: Glob pattern for files to watch (e.g., "styles/**/*.scss").
    ///
    /// # Returns
    ///
    /// A [`Handle`] to a [`crate::loader::Assets`] mapping original file paths to [`Stylesheet`] objects.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # let mut config = hauchiwa::Blueprint::<()>::new();
    /// // Compile main.scss, watching all scss files in the styles directory for changes.
    /// let styles = config.load_css("styles/main.scss", "styles/**/*.scss");
    /// ```
    pub fn load_css(
        &mut self,
        glob_entry: &'static str,
        glob_watch: &'static str,
    ) -> Result<Handle<super::Assets<Stylesheet>>, HauchiwaError> {
        Ok(self.add_task_opaque(GlobAssetsTask::new(
            vec![glob_entry],
            vec![glob_watch],
            move |_, store, input| {
                let data = grass::from_path(&input.path, &grass::Options::default())
                    .map_err(StyleError::Sass)?;

                let path = store
                    .save(data.as_bytes(), "css")
                    .map_err(StyleError::Build)?;

                Ok((input.path, Stylesheet { path }))
            },
        )?))
    }
}
