use thiserror::Error;

use crate::{SiteConfig, error::HauchiwaError, loader::glob::GlobRegistryTask, task::Handle};

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
pub struct CSS {
    /// The path to the compiled CSS file.
    pub path: camino::Utf8PathBuf,
}

impl<G> SiteConfig<G>
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
    /// A handle to a registry mapping original file paths to `CSS` objects.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let styles = config.load_css("styles/main.scss", "styles/**/*.scss")?;
    /// ```
    pub fn load_css(
        &mut self,
        glob_entry: &'static str,
        glob_watch: &'static str,
    ) -> Result<Handle<super::Registry<CSS>>, HauchiwaError> {
        Ok(self.add_task_opaque(GlobRegistryTask::new(
            vec![glob_entry],
            vec![glob_watch],
            move |_, rt, file| {
                let data = grass::from_path(&file.path, &grass::Options::default())
                    .map_err(StyleError::Sass)?;

                let path = rt
                    .store(data.as_bytes(), "css")
                    .map_err(StyleError::Build)?;

                Ok((file.path, CSS { path }))
            },
        )?))
    }
}
