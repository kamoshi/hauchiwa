use camino::Utf8PathBuf;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::{
    Blueprint, Environment,
    error::HauchiwaError,
    loader::{GlobAssetsTask, Input, Store},
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

/// This is the standard output of the [`SiteConfig::load_documents`] loader.
///
/// # Generics
///
/// * `T`: The type of the metadata (frontmatter), typically a struct deriving `Deserialize`.
#[derive(Clone)]
pub struct Document<T> {
    /// The parsed metadata (frontmatter).
    pub metadata: T,
    /// The original path of content file.
    pub path: Utf8PathBuf,
    /// The body content of the file (excluding frontmatter).
    pub body: String,
}

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Registers a generic asset loader.
    ///
    /// This method allows you to process files matching a glob pattern using a
    /// custom closure. It is useful for loading assets that don't fit into
    /// standard categories (like simple text files, JSON data, or binary
    /// assets).
    ///
    /// # Type Parameters
    ///
    /// * `R`: The return type of the callback, which will be stored in the Registry.
    ///
    /// # Arguments
    ///
    /// * `path_glob` - A glob pattern to find files (e.g., `"assets/**/*.json"`).
    /// * `callback` - A function that takes the global context, a runtime
    ///   handle, and the raw file. It should return the processed data.
    ///
    /// # Returns
    ///
    /// A `Handle` to a `Registry<R>`, mapping file paths to your processed data.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Load all .txt files and reverse their content.
    /// let reversed_texts = config.load("content/**/*.txt", |globals, file| {
    ///     let content = String::from_utf8(file.data.into())?;
    ///     let reversed = content.chars().rev().collect::<String>();
    ///     Ok(reversed)
    /// })?;
    /// ```
    pub fn load<R>(
        &mut self,
        path_glob: &'static str,
        callback: impl Fn(&Environment<G>, &mut Store, Input) -> anyhow::Result<R>
        + Send
        + Sync
        + 'static,
    ) -> Result<Handle<super::Assets<R>>, HauchiwaError>
    where
        G: Send + Sync + 'static,
        R: Send + Sync + 'static,
    {
        Ok(self.add_task_opaque(GlobAssetsTask::new(
            vec![path_glob],
            vec![path_glob],
            move |ctx, rt, file| {
                let path = file.path.clone();
                let data = callback(ctx.env, rt, file)?;

                Ok((path, data))
            },
        )?))
    }

    /// Registers a content loader that parses frontmatter.
    ///
    /// This is the primary way to load Markdown (or other text) files that contain
    /// YAML frontmatter. The loader will:
    /// 1. Read the file as UTF-8.
    /// 2. Extract and parse the YAML frontmatter into type `R`.
    /// 3. Return the frontmatter and the remaining body content.
    ///
    /// # Type Parameters
    ///
    /// * `R`: The type to deserialize the frontmatter into. Must implement `serde::Deserialize`.
    ///
    /// # Arguments
    ///
    /// * `path_glob` - A glob pattern to find files (e.g., `"content/posts/**/*.md"`).
    ///
    /// # Returns
    ///
    /// A `Handle` to a `Registry<Content<R>>`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// #[derive(serde::Deserialize, Clone)]
    /// struct Post {
    ///     title: String,
    ///     date: String,
    /// }
    ///
    /// // Load all markdown files in the posts directory, parsing their
    /// // frontmatter into PostMeta structs.
    /// let posts = config.load_frontmatter::<Post>("content/posts/*.md")?;
    /// ```
    pub fn load_documents<R>(
        &mut self,
        path_glob: &'static str,
    ) -> Result<Handle<super::Assets<Document<R>>>, HauchiwaError>
    where
        G: Send + Sync + 'static,
        R: DeserializeOwned + Send + Sync + 'static,
    {
        Ok(self.add_task_opaque(GlobAssetsTask::new(
            vec![path_glob],
            vec![path_glob],
            move |_, _, file: Input| {
                let data = std::str::from_utf8(&file.data).map_err(FrontmatterError::Utf8)?;
                let (metadata, content) =
                    super::parse_yaml::<R>(data).map_err(FrontmatterError::Parse)?;

                Ok((
                    file.path.clone(),
                    Document {
                        path: file.path,
                        metadata,
                        body: content,
                    },
                ))
            },
        )?))
    }
}
