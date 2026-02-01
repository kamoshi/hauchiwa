#![doc = include_str!("../README.md")]
#![deny(
    unsafe_code,
    // clippy::unwrap_used,
    // clippy::expect_used,
    clippy::panic,
)]

mod blueprint;
mod core;
mod engine;
pub mod error;
mod executor;
mod graph;
pub mod importmap;
pub mod loader;
pub mod output;
mod utils;

use std::fmt::Debug;

use camino::Utf8PathBuf;

pub use camino;
pub use gitscan as git;
pub use tracing::{debug, error, info, trace, warn};

pub use crate::blueprint::{Blueprint, Website};
pub use crate::core::Environment;
pub use crate::engine::{HandleC, HandleF, Tracker};
pub use crate::executor::Diagnostics;
pub use crate::importmap::ImportMap;
pub use crate::loader::Store;
pub use crate::output::Output;

// pub use crate::core::{Environment, Mode};

/// The context passed to every task execution.
///
/// `TaskContext` provides access to global settings and the aggregated import
/// map from all dependencies. It is immutable during task execution.
pub struct TaskContext<'a, G: Send + Sync = ()> {
    /// Access to global configuration and data.
    pub env: &'a Environment<G>,
    /// The current import map, containing JavaScript module mappings from all
    /// upstream dependencies.
    pub importmap: &'a ImportMap,
    /// Tracing span assigned to this task.
    span: tracing::Span,
}

#[derive(Debug)]
pub struct FileMetadata {
    pub file: Utf8PathBuf,
    pub area: Utf8PathBuf,
    pub info: Option<crate::git::GitInfo>,
}
