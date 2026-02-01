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

pub use camino;
pub use gitscan as git;

pub use crate::blueprint::{Blueprint, Website};
pub use crate::core::{Environment, FileMetadata, ImportMap, Mode, Store, TaskContext};
pub use crate::engine::{Diagnostics, Many, One, Tracker};
pub use crate::output::Output;

pub mod prelude {
    pub use super::blueprint::{Blueprint, Website};
    pub use super::core::{Store, TaskContext};
    pub use super::engine::{Many, One};
    pub use super::output::Output;
}

pub mod tracing {
    pub use tracing::{debug, error, info, trace, warn};
}
