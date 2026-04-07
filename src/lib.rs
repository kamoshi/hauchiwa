#![doc = include_str!("../README.md")]
#![deny(unsafe_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod blueprint;
mod core;
mod engine;
pub mod error;
pub mod loader;
#[cfg(feature = "logging")]
mod logging;
pub mod output;
pub(crate) mod snapshot;
mod utils;

pub use camino;
pub use gitscan as git;

#[cfg(feature = "minijinja")]
pub mod minijinja {
    pub use ::minijinja::context;
}

pub use crate::blueprint::{Blueprint, Website};
pub use crate::core::{Environment, FileMetadata, ImportMap, Mode, Store, TaskContext};
pub use crate::engine::{Diagnostics, Many, One, Tracker};
pub use crate::output::Output;

pub mod prelude {
    pub use super::blueprint::{Blueprint, Website};
    pub use super::core::{ImportMap, Store, TaskContext};
    pub use super::engine::{Diagnostics, Many, One, Tracker};
    pub use super::output::Output;
}

pub mod tracing {
    pub use tracing::{debug, error, info, trace, warn};
}

#[cfg(feature = "logging")]
pub use logging::init_logging;
