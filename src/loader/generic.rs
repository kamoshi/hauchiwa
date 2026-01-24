use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::{
    Blueprint, Environment, Output,
    error::HauchiwaError,
    graph::Handle,
    loader::{GlobAssetsTask, Input, Store},
    page::OutputBuilder,
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

/// This is the standard output of the [`Blueprint::load_documents`] loader.
///
/// # Generics
///
/// * `T`: The type of the metadata (frontmatter), typically a struct deriving `Deserialize`.
#[derive(Clone)]
pub struct Document<T> {
    /// The parsed frontmatter.
    pub matter: Box<T>,
    /// The body content of the file (excluding frontmatter).
    pub text: String,
    /// Metadata related to the document.
    pub meta: DocumentMeta,
}

#[derive(Debug, Clone)]
pub struct DocumentMeta {
    /// The original path of content file.
    pub path: Utf8PathBuf,
    /// The shared offset path used to calculate the href.
    pub offset: Option<Arc<str>>,
    /// The web-accessible URL path.
    pub href: String,
}

impl DocumentMeta {
    /// Extracts the "slug" or distinct identifier for the document.
    ///
    /// * If the file is `.../some-file.md`, returns "some-file".
    /// * If the file is `.../some-file/index.md`, returns "some-file".
    pub fn slug(&self) -> &str {
        let stem = self.path.file_stem().unwrap_or_default();

        if stem == "index" {
            // If it is an index file, the "identity" is the parent folder name
            self.path
                .parent()
                .and_then(|p| p.file_name())
                .unwrap_or(stem)
        } else {
            stem
        }
    }

    /// Calculates the final output file path for the built artifact.
    ///
    /// This converts both `foo.md` and `foo/index.md` into `dist/.../foo/index.html`.
    pub fn dist_path(&self, out: impl AsRef<Utf8Path>) -> Utf8PathBuf {
        crate::page::href_to_dist(&self.href, out)
    }

    /// Generates a glob pattern for assets relative to this document.
    ///
    /// This simplifies selecting co-located assets. For example:
    /// - If the document is `posts/hello.md`, `.assets("*.bib")` returns `"posts/hello/*.bib"`.
    /// - If the document is `posts/hello/index.md`, it also returns `"posts/hello/*.bib"`.
    pub fn assets(&self, pattern: &str) -> String {
        // Get the bundle scope (base directory) of the document
        let base = crate::page::source_to_bundle(&self.path);

        // Join the pattern with the base directory
        base.join(pattern).to_string()
    }

    /// Resolves a relative path against the document's source location.
    ///
    /// This is useful for processing links inside the content, such as
    /// `[Link](../other.md)` or `![Image](./img.png)`.
    ///
    /// # Example
    /// - Doc: `content/posts/hello/index.md`
    /// - Input: `../world.md`
    /// - Result: `content/posts/world.md`
    pub fn resolve(&self, path: impl AsRef<str>) -> Utf8PathBuf {
        // Get the bundle scope (base directory) of the document
        let base = crate::page::source_to_bundle(&self.path);

        // Join the relative path (e.g. "../foo.png")
        let joined = base.join(path.as_ref());

        // Normalize to remove ".." and "." components
        crate::page::normalize_path(&joined)
    }
}

impl<T> Document<T> {
    pub fn output(&self) -> OutputBuilder {
        Output::mapper(&self.meta.path)
    }
}

/// A builder for configuring the document loader task.
pub struct DocumentLoader<'a, G, R>
where
    G: Send + Sync,
    R: DeserializeOwned + Send + Sync + 'static,
{
    blueprint: &'a mut Blueprint<G>,
    sources: Vec<&'static str>,
    offset: Option<String>,
    _phantom: std::marker::PhantomData<R>,
}

impl<'a, G, R> DocumentLoader<'a, G, R>
where
    G: Send + Sync + 'static,
    R: DeserializeOwned + Send + Sync + 'static,
{
    fn new(blueprint: &'a mut Blueprint<G>) -> Self {
        Self {
            blueprint,
            sources: Vec::new(),
            offset: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Adds a glob pattern to find documents.
    pub fn source(mut self, glob: &'static str) -> Self {
        self.sources.push(glob);
        self
    }

    /// Sets the offset path for the documents.
    ///
    /// This path will be stripped from the file path when calculating the `href`.
    pub fn offset(mut self, offset: impl Into<String>) -> Self {
        self.offset = Some(offset.into());
        self
    }

    /// Registers the task with the Blueprint.
    pub fn register(self) -> Result<Handle<super::Assets<Document<R>>>, HauchiwaError> {
        let offset = self.offset.map(Arc::from);

        Ok(self.blueprint.add_task_opaque(GlobAssetsTask::new(
            self.sources.clone(),
            self.sources,
            move |_, _, input: Input| {
                let bytes = input
                    .read()
                    .map_err(|e| FrontmatterError::Parse(e.into()))?;

                let data = std::str::from_utf8(&bytes).map_err(FrontmatterError::Utf8)?;

                let (metadata, content) =
                    super::parse_yaml::<R>(data).map_err(FrontmatterError::Parse)?;

                let href = crate::page::source_to_href(&input.path, offset.as_deref());

                Ok((
                    input.path.clone(),
                    Document {
                        matter: Box::new(metadata),
                        text: content,
                        meta: DocumentMeta {
                            path: input.path,
                            offset: offset.clone(),
                            href,
                        },
                    },
                ))
            },
        )?))
    }
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
    /// * `R`: The return type of the callback, which will be stored in [`crate::loader::Assets`].
    ///
    /// # Arguments
    ///
    /// * `path_glob` - A glob pattern to find files (e.g., `"assets/**/*.json"`).
    /// * `callback` - A function that takes the task context, a store handle,
    ///   and the input. It should return the processed data.
    ///
    /// # Returns
    ///
    /// A [`Handle`] to a [`crate::loader::Assets<R>`], mapping file paths to your processed data.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # let mut config = hauchiwa::Blueprint::<()>::new();
    /// // Load all .txt files and reverse their content.
    /// let reversed_texts = config.load("content/**/*.txt", |_, _, input| {
    ///     let content = String::from_utf8(input.read()?.to_vec())?;
    ///     let reversed = content.chars().rev().collect::<String>();
    ///     Ok(reversed)
    /// });
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
            move |ctx, store, input| {
                let path = input.path.clone();
                let data = callback(ctx.env, store, input)?;

                Ok((path, data))
            },
        )?))
    }

    /// Starts configuring a document loader task.
    ///
    /// This is the primary way to load Markdown (or other text) files that contain
    /// YAML frontmatter.
    ///
    /// # Type Parameters
    ///
    /// * `R`: The type to deserialize the frontmatter into. Must implement [`serde::Deserialize`].
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// #[derive(serde::Deserialize, Clone)]
    /// struct Post {
    ///     title: String,
    ///     date: String,
    /// }
    ///
    /// # let mut config = hauchiwa::Blueprint::<()>::new();
    /// // Load all markdown files in the posts directory, parsing their
    /// // frontmatter into PostMeta structs.
    /// let posts = config.load_documents::<Post>()
    ///     .source("content/posts/*.md")
    ///     .register();
    /// ```
    pub fn load_documents<R>(&mut self) -> DocumentLoader<'_, G, R>
    where
        G: Send + Sync + 'static,
        R: DeserializeOwned + Send + Sync + 'static,
    {
        DocumentLoader::new(self)
    }
}
