use camino::Utf8PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HauchiwaError {
    #[error(transparent)]
    Loader(#[from] LoaderError),

    #[error("Encountered an error while clearing the dist directory:\n{0}")]
    Clear(#[from] ClearError),

    #[error("Error while cloning static content: {0}")]
    CloneStatic(std::io::Error),

    #[error("Error while building sitemap {0}")]
    Sitemap(#[from] SitemapError),

    #[error("Failed to compile stylesheets: {0}")]
    Stylesheet(#[from] StylesheetError),

    #[error("Error while watching {0}")]
    Watch(#[from] WatchError),

    #[error("Invalid glob pattern: {0}")]
    Glob(#[from] glob::PatternError),

    #[error("Asset not found: {0}")]
    AssetNotFound(String),

    #[error("Frontmatter has wrong shape: {0}")]
    Frontmatter(String),

    #[error("Failed to acquire read lock")]
    LockRead,

    #[error("Failed to acquire write lock")]
    LockWrite,

    #[error("Failed to build asset {0}")]
    Builder(#[from] BuilderError),

    #[error("Error while executing a hook:\n{0}")]
    Hook(#[from] HookError),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct LoaderFileCallbackError(pub anyhow::Error);

#[derive(Debug, Error)]
pub enum LoaderFileError {
    #[error(transparent)]
    Callback(#[from] LoaderFileCallbackError),

    #[error(transparent)]
    FileSystem(#[from] std::io::Error),

    #[error("Error while reading frontmatter - {0}")]
    Frontmatter(String),
}

#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("Encountered an error while loading file {0}:\n{1}")]
    LoaderGlobFile(Utf8PathBuf, LoaderFileError),

    #[error(transparent)]
    GlobPattern(#[from] glob::PatternError),

    #[error(transparent)]
    Glob(#[from] glob::GlobError),

    #[error(transparent)]
    PathFormat(#[from] camino::FromPathBufError),
}

#[derive(Debug, Error)]
pub enum ClearError {
    #[error(transparent)]
    RemoveError(std::io::Error),

    #[error(transparent)]
    CreateError(std::io::Error),
}

#[derive(Debug, Error)]
pub enum SitemapError {
    #[error("Failed to write output to file {0}")]
    FileWrite(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum StylesheetError {
    #[error("Glob pattern error: {0}")]
    GlobPattern(#[from] glob::PatternError),

    #[error("Glob error: {0}")]
    Glob(#[from] glob::GlobError),

    #[error("Invalid file name, only UTF-8 filenames are supported. {0}")]
    InvalidFileName(String),

    #[error("CSS compile error: {0}")]
    Compiler(String),
}

#[derive(Debug, Error)]
pub enum WatchError {
    #[error("Failed to bind to address {0}")]
    Bind(std::io::Error),

    #[error(transparent)]
    Loader(#[from] LoaderError),
}

#[derive(Debug, Error)]
pub enum BuilderError {
    #[error("Userland error: {0}")]
    Userland(#[from] anyhow::Error),

    #[error("Failed to read file `{0}`: {1}")]
    FileReadError(Utf8PathBuf, std::io::Error),

    #[error("Failed to create directory `{0}`: {1}")]
    CreateDirError(Utf8PathBuf, std::io::Error),

    #[error("Failed to write file `{0}`: {1}")]
    FileWriteError(Utf8PathBuf, std::io::Error),

    #[error("Failed to copy file from `{0}` to `{1}`: {2}")]
    FileCopyError(Utf8PathBuf, Utf8PathBuf, std::io::Error),

    #[error("Failed to optimize image")]
    OptimizationError,
}

#[derive(Debug, Error)]
pub enum HookError {
    #[error(transparent)]
    Userland(#[from] anyhow::Error),
}
