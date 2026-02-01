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
pub mod loader;
pub mod output;
mod utils;

use std::fmt::Debug;

use camino::Utf8PathBuf;

pub use camino;
pub use gitscan as git;
pub use tracing::{debug, error, info, trace, warn};

pub use crate::blueprint::{Blueprint, Website};
pub use crate::core::{Environment, ImportMap, Mode, TaskContext};
pub use crate::engine::{Diagnostics, HandleC, HandleF, Tracker};
pub use crate::output::Output;

#[derive(Debug)]
pub struct FileMetadata {
    pub file: Utf8PathBuf,
    pub area: Utf8PathBuf,
    pub info: Option<crate::git::GitInfo>,
}
