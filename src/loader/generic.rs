use camino::Utf8PathBuf;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::{
    Globals, SiteConfig,
    error::HauchiwaError,
    loader::{File, Runtime, glob::GlobRegistryTask},
    task::Handle,
};

/// Errors that can occur when loading files with frontmatter.
#[derive(Debug, Error)]
pub enum FrontmatterError {
    /// Failed to convert file content to UTF-8.
    #[error("UTF-8 conversion error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    /// Failed to parse the frontmatter of the content file.
    #[error("Frontmatter parsing error: {0}")]
    Parse(anyhow::Error),
}

/// Represents a loaded content file with parsed metadata and body content.
///
/// # Generics
///
/// * `T`: The type of the metadata (frontmatter).
#[derive(Clone)]
pub struct Content<T> {
    /// The original path of the content file.
    pub path: Utf8PathBuf,
    /// The parsed metadata (frontmatter).
    pub metadata: T,
    /// The body content of the file (excluding frontmatter).
    pub content: String,
}

impl<G> SiteConfig<G>
where
    G: Send + Sync + 'static,
{
    /// Add a task that finds files matching a glob pattern, processes each file
    /// using a provided callback, and collects the data into a `Registry`.
    ///
    /// # Parameters
    ///
    /// * `site_config`: The mutable `SiteConfig` to which the new task will be added.
    /// * `path_glob`: A glob pattern (e.g., `"static/**/*"`) used to find files. This
    ///   pattern is used for both the initial file discovery and for watching for changes.
    /// * `callback`: A closure that defines the processing for each file. It receives
    ///   the `&Globals<G>` and a `File<Vec<u8>>` (containing the file's path and
    ///   raw byte content) and must return an `anyhow::Result<R>`.
    ///
    /// # Generics
    ///
    /// * `G`: The type of the global data.
    /// * `R`: The return type of the `callback` for a single file. This is the
    ///   value that will be stored in the `Registry`.
    ///
    /// # Returns
    ///
    /// Returns a `Handle<super::Registry<R>>`, which is a typed reference to the
    /// task's output in the build graph. The output will be the `Registry`
    /// containing all processed file results.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // A simple loader that reads files as UTF-8 strings.
    /// let text_files = config.load("content/**/*.txt", |_, _, file| {
    ///     let content = String::from_utf8(file.metadata)?;
    ///     Ok(content)
    /// })?;
    /// ```
    pub fn load<R>(
        &mut self,
        path_glob: &'static str,
        callback: impl Fn(&Globals<G>, &mut Runtime, File<Vec<u8>>) -> anyhow::Result<R>
        + Send
        + Sync
        + 'static,
    ) -> Result<Handle<super::Registry<R>>, HauchiwaError>
    where
        G: Send + Sync + 'static,
        R: Send + Sync + 'static,
    {
        Ok(self.add_task_opaque(GlobRegistryTask::new(
            vec![path_glob],
            vec![path_glob],
            move |ctx, rt, file| {
                let path = file.path.clone();
                let data = callback(ctx.globals, rt, file)?;

                Ok((path, data))
            },
        )?))
    }

    /// Scans for content files matching a glob pattern and parses their frontmatter.
    ///
    /// This loader reads files, treats them as UTF-8, and parses optional YAML frontmatter
    /// at the beginning of the file. The remaining content is treated as the body.
    ///
    /// # Type Parameters
    ///
    /// * `G`: The global site context type.
    /// * `R`: The type of the metadata (frontmatter) to deserialize into.
    ///
    /// # Arguments
    ///
    /// * `site_config`: The site configuration builder.
    /// * `path_glob`: The glob pattern to find files (e.g., "posts/*.md").
    ///
    /// # Returns
    ///
    /// A handle to a registry mapping file paths to `Content<R>` objects.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// #[derive(serde::Deserialize, Clone)]
    /// struct PostMeta {
    ///     title: String,
    ///     date: String,
    /// }
    ///
    /// // Load all markdown files in the posts directory, parsing their
    /// // frontmatter into PostMeta structs.
    /// let posts = config.load_frontmatter::<PostMeta>("content/posts/*.md")?;
    /// ```
    pub fn load_frontmatter<R>(
        &mut self,
        path_glob: &'static str,
    ) -> Result<Handle<super::Registry<Content<R>>>, HauchiwaError>
    where
        G: Send + Sync + 'static,
        R: DeserializeOwned + Send + Sync + 'static,
    {
        Ok(self.add_task_opaque(GlobRegistryTask::new(
            vec![path_glob],
            vec![path_glob],
            move |_, _, file: File<Vec<u8>>| {
                let data = std::str::from_utf8(&file.metadata).map_err(FrontmatterError::Utf8)?;
                let (metadata, content) =
                    super::parse_yaml::<R>(data).map_err(FrontmatterError::Parse)?;

                Ok((
                    file.path.clone(),
                    Content {
                        path: file.path,
                        metadata,
                        content,
                    },
                ))
            },
        )?))
    }
}
