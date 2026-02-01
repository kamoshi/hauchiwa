use std::any::Any;
use std::sync::Arc;

/// A type-erased, thread-safe container.
pub(crate) type Dynamic = Arc<dyn Any + Send + Sync>;

/// A 32-byte BLAKE3 hash used for content-addressing and change detection.
///
/// In `hauchiwa`, this serves two primary purposes:
/// 1. It acts as a unique fingerprint for task inputs and outputs to determine
///    if they are "dirty" and require rebuilding.
/// 2. It generates unique filenames (e.g., inside `dist/hash/`) for assets like
///    images or scripts, ensuring effective browser caching.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) struct Hash32([u8; 32]);

impl<T> From<T> for Hash32
where
    T: Into<[u8; 32]>,
{
    fn from(value: T) -> Self {
        Hash32(value.into())
    }
}

impl Hash32 {
    pub(crate) fn hash(buffer: impl AsRef<[u8]>) -> Self {
        blake3::Hasher::new()
            .update(buffer.as_ref())
            .finalize()
            .into()
    }

    pub(crate) fn hash_file(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        Ok(blake3::Hasher::new().update_mmap(path)?.finalize().into())
    }

    pub(crate) fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut acc = vec![0u8; 64];

        for (i, &byte) in self.0.iter().enumerate() {
            acc[i * 2] = HEX[(byte >> 4) as usize];
            acc[i * 2 + 1] = HEX[(byte & 0xF) as usize];
        }

        String::from_utf8(acc).unwrap()
    }
}

impl std::fmt::Debug for Hash32 {
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
