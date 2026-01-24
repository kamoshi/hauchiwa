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
    /// The parsed metadata (frontmatter).
    pub metadata: Box<T>,
    /// The original path of content file.
    pub path: Utf8PathBuf,
    /// The body content of the file (excluding frontmatter).
    pub body: String,
    /// The shared offset path used to calculate the href.
    pub offset: Option<Arc<str>>,
    /// The web-accessible URL path.
    pub href: String,
}

impl<T> Document<T> {
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
        // Remove leading slash to join correctly with dist_dir
        out.as_ref()
            .join(self.href.trim_start_matches('/'))
            .join("index.html")
    }

    pub fn output(&self) -> OutputBuilder {
        Output::mapper(&self.path)
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

                let href = compute_href(&input.path, offset.as_deref());

                Ok((
                    input.path.clone(),
                    Document {
                        metadata: Box::new(metadata),
                        path: input.path,
                        body: content,
                        offset: offset.clone(),
                        href,
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

/// Helper function to compute the web-accessible URL path (href).
fn compute_href(path: &Utf8Path, offset: Option<&str>) -> String {
    let path = if let Some(offset) = offset {
        path.strip_prefix(offset).unwrap_or(path)
    } else {
        path
    };

    let mut url = String::from("/");

    // If it's not index.md, we need to append the stem (e.g., 'some-file')
    // If it IS index.md, we only want the parent directory structure.
    if let Some(parent) = path.parent() {
        url.push_str(parent.as_str());
    }

    let stem = path.file_stem().unwrap_or_default();
    if stem != "index" {
        if !url.ends_with('/') {
            url.push('/');
        }
        url.push_str(stem);
    }

    // Ensure trailing slash for directory-style routing
    if !url.ends_with('/') {
        url.push('/');
    }

    // Handling edge case: double slash at start if parent was empty
    if url.starts_with("//") {
        url.replace("//", "/")
    } else {
        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_href() {
        // Simple file
        assert_eq!(
            compute_href(Utf8Path::new("content/posts/hello.md"), Some("content")),
            "/posts/hello/"
        );

        // Index file
        assert_eq!(
            compute_href(Utf8Path::new("content/posts/index.md"), Some("content")),
            "/posts/"
        );

        // No offset
        assert_eq!(
            compute_href(Utf8Path::new("posts/hello.md"), None),
            "/posts/hello/"
        );

        // Root index
        assert_eq!(compute_href(Utf8Path::new("index.md"), None), "/");

        // Double slash edge case (e.g. parent is empty after offset strip, but it's not index)
        assert_eq!(
            compute_href(Utf8Path::new("content/hello.md"), Some("content")),
            "/hello/"
        );

        // Double slash edge case with index
        assert_eq!(
            compute_href(Utf8Path::new("content/index.md"), Some("content")),
            "/"
        );

        // Deeply nested
        assert_eq!(
            compute_href(Utf8Path::new("content/a/b/c.md"), Some("content")),
            "/a/b/c/"
        );
    }
}
