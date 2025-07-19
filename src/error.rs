use std::sync::Arc;
#[cfg(feature = "notify")]
use std::sync::mpsc::{RecvError, SendError};

use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error(transparent)]
pub struct LazyAssetError(#[from] pub(crate) Arc<anyhow::Error>);

impl LazyAssetError {
    pub(crate) fn new(err: impl Into<anyhow::Error>) -> Self {
        Self(Arc::new(err.into()))
    }
}

impl From<anyhow::Error> for LazyAssetError {
    fn from(e: anyhow::Error) -> Self {
        LazyAssetError(Arc::new(e))
    }
}

#[derive(Debug, Error)]
pub enum HauchiwaError {
    #[error(transparent)]
    AnyhowArc(#[from] Arc<anyhow::Error>),

    #[error("Asset '{0}': {1}")]
    Asset(Box<str>, LazyAssetError),

    #[error("Loader '{0}': {1}")]
    Loader(String, LoaderError),

    #[error("Error while clearing the dist directory:\n{0}")]
    StepClear(#[from] StepClearError),

    #[error("Error while copying static content:\n{0}")]
    StepStatic(#[from] StepCopyStatic),

    #[error("Error while building the website.\n{0}")]
    Build(#[from] BuildError),

    #[cfg(feature = "notify")]
    #[error("Error while watching for file changes:\n{0}")]
    Watch(#[from] WatchError),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct LoaderFileCallbackError(pub anyhow::Error);

#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("Couldn't load data from file.\n{0}")]
    FileSystem(#[from] std::io::Error),

    #[error("Couldn't compile glob pattern.\n{0}")]
    GlobPattern(#[from] glob::PatternError),

    #[error("Couldn't run glob.\n{0}")]
    Glob(#[from] glob::GlobError),

    #[error("Couldn't convert path to UTF-8.\n{0}")]
    PathFormat(#[from] camino::FromPathBufError),

    #[error("An error occured while loading asset.\n{0}")]
    Userland(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct StepClearError(#[from] std::io::Error);

#[derive(Debug, Error)]
#[error(transparent)]
pub struct StepCopyStatic(#[from] std::io::Error);

#[derive(Debug, Error)]
pub enum BuildError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Task '{0}':\n{1}")]
    Task(String, anyhow::Error),

    #[error("Hook:\n{0}")]
    Hook(anyhow::Error),
}

#[cfg(feature = "notify")]
#[derive(Debug, Error)]
pub enum WatchError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Build(#[from] BuildError),

    #[error(transparent)]
    Notify(#[from] notify::Error),

    #[error(transparent)]
    Recv(#[from] RecvError),

    #[error(transparent)]
    Send(#[from] SendError<()>),
}

#[derive(Debug, Error)]
pub enum HookError {
    #[error(transparent)]
    Userland(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum ContextError {
    #[error(transparent)]
    Pattern(#[from] glob::PatternError),

    #[error("Asset not found: {0}")]
    NotFound(String),

    #[error("Asset not found: {0}, available assets with types {1}")]
    NotFoundWrongShape(String, String),

    #[error("Asset {0}:\n{1}")]
    LazyAssetError(String, LazyAssetError),
}
