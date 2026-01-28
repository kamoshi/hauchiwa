#![doc = include_str!("../README.md")]
#![deny(
    unsafe_code,
    // clippy::unwrap_used,
    // clippy::expect_used,
    clippy::panic,
)]

mod blueprint;
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
pub use crate::executor::Diagnostics;
pub use crate::graph::Handle;
pub use crate::importmap::ImportMap;
pub use crate::loader::Store;
pub use crate::output::Output;

/// 32 bytes length generic hash
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
struct Hash32([u8; 32]);

impl<T> From<T> for Hash32
where
    T: Into<[u8; 32]>,
{
    fn from(value: T) -> Self {
        Hash32(value.into())
    }
}

impl Hash32 {
    fn hash(buffer: impl AsRef<[u8]>) -> Self {
        blake3::Hasher::new()
            .update(buffer.as_ref())
            .finalize()
            .into()
    }

    fn hash_file(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        Ok(blake3::Hasher::new().update_mmap(path)?.finalize().into())
    }

    fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut acc = vec![0u8; 64];

        for (i, &byte) in self.0.iter().enumerate() {
            acc[i * 2] = HEX[(byte >> 4) as usize];
            acc[i * 2 + 1] = HEX[(byte & 0xF) as usize];
        }

        String::from_utf8(acc).unwrap()
    }
}

impl Debug for Hash32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hash32({})", self.to_hex())
    }
}

/// The mode in which the site generator is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A one-time build.
    Build,
    /// A continuous watch mode for development.
    Watch,
}

/// Global configuration and state available to all tasks.
///
/// This struct allows you to share global data (like configuration options or
/// shared state) across your entire task graph.
///
/// # Type Parameters
///
/// * `G`: The type of the user-defined global data. Must be `Send + Sync`.
#[derive(Clone)]
pub struct Environment<D: Send + Sync = ()> {
    /// The name of the generator (defaults to "hauchiwa").
    pub generator: &'static str,
    /// The current build mode (Build or Watch).
    pub mode: Mode,
    /// The port of the development server (if running).
    pub port: Option<u16>,
    /// User-defined global data.
    pub data: D,
}

impl<G: Send + Sync> Environment<G> {
    /// Returns a JavaScript snippet to enable live-reloading.
    ///
    /// If the site is running in `Watch` mode and a port is configured, this returns
    /// a script that connects to the WebSocket server to listen for reload events.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use hauchiwa::{Blueprint, task};
    /// # let mut config = Blueprint::<()>::default();
    /// # task!(config, |ctx| {
    /// let script = ctx.env.get_refresh_script();
    /// if let Some(s) = script {
    ///     // Inject `s` into your HTML <head> or <body>
    /// }
    /// # Ok(())
    /// # });
    /// ```
    pub fn get_refresh_script(&self) -> Option<String> {
        self.port.map(|port| {
            format!(
                r#"
const socket = new WebSocket("ws://localhost:{port}");
socket.addEventListener("message", event => {{
    window.location.reload();
}});
"#
            )
        })
    }
}

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
