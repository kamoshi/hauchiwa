use std::fs;
use std::sync::Arc;
use std::{any::Any, collections::BTreeMap};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::error::BuildError;

/// A type-erased, thread-safe container.
pub(crate) type Dynamic = Arc<dyn Any + Send + Sync>;

/// Atomic reference-counted string type used for identifiers.
pub(crate) type ArcStr = std::sync::Arc<str>;

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

#[derive(Default)]
pub(crate) struct Blake3Hasher(blake3::Hasher);

impl From<Blake3Hasher> for Hash32 {
    fn from(value: Blake3Hasher) -> Self {
        let bytes: [u8; 32] = value.0.finalize().into();
        Hash32::from(bytes)
    }
}

impl std::hash::Hasher for Blake3Hasher {
    fn finish(&self) -> u64 {
        let mut output = [0u8; 8];
        self.0.finalize_xof().fill(&mut output);
        u64::from_le_bytes(output)
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
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

impl<G: Send + Sync> std::fmt::Debug for Environment<G>
where
    G: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment")
            .field("generator", &self.generator)
            .field("mode", &self.mode)
            .field("port", &self.port)
            .field("data", &self.data)
            .finish()
    }
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
    /// # use hauchiwa::Blueprint;
    /// # let mut config = Blueprint::<()>::default();
    /// config.task().run(|ctx| {
    ///     let script = ctx.env.get_refresh_script();
    ///     if let Some(s) = script {
    ///         // Inject `s` into your HTML <head> or <body>
    ///     }
    ///     Ok(())
    /// });
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

/// This matches the browser's Import Map specification.
/// <https://developer.mozilla.org/en-US/docs/Web/HTML/Element/script/type/importmap>
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ImportMap {
    imports: BTreeMap<String, String>,
}

impl ImportMap {
    /// Creates a new, empty ImportMap
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new module key and its path.
    ///
    /// # Arguments
    /// * `key` - The module specifier (e.g., "svelte")
    /// * `value` - The URL or path (e.g., "/_app/svelte.js")
    pub fn register(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.imports.insert(key.into(), value.into());
        self
    }

    /// Merges another import map into this one.
    /// Entries from `other` will overwrite entries in `self` if keys conflict.
    pub fn merge(&mut self, other: ImportMap) {
        for (key, value) in other.imports {
            self.imports.insert(key, value);
        }
    }

    /// Serialize the map to a JSON string.
    pub fn to_json(&self) -> serde_json::Result<String> {
        // "pretty" is optional; strictly minified is fine too.
        serde_json::to_string(self)
    }

    /// Serialize the importmap to a proper HTML script tag importmap.
    pub fn to_html(&self) -> serde_json::Result<String> {
        self.to_json()
            .map(|json| format!(r#"<script type="importmap">{json}</script>"#))
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
    pub(crate) span: tracing::Span,
}

/// A helper for managing side effects and imports within a task.
///
/// `Store` is passed to task callbacks to allow them to:
/// 1. Store generated artifacts (like optimized images or compiled CSS) to the `dist` directory.
/// 2. Register module imports (for the Import Map) that this task introduces.
///
/// It handles content-addressable storage (hashing) automatically to ensure caching works correctly.
#[derive(Clone)]
pub struct Store {
    pub(crate) imports: ImportMap,
}

impl Store {
    /// Creates a new, empty Store.
    pub fn new() -> Self {
        Self {
            imports: ImportMap::new(),
        }
    }

    /// Saves raw data as a content-addressed artifact.
    ///
    /// The data is hashed, and the file is stored at `/hash/<hash>.<ext>`.
    ///
    /// # Arguments
    ///
    /// * `data` - The raw bytes to store.
    /// * `ext` - The file extension for the stored file (e.g., "png", "css").
    ///
    /// # Returns
    ///
    /// The logical path to the file (e.g., `/hash/abcdef123.png`), suitable for use in HTML `src` attributes.
    pub fn save(&self, data: &[u8], ext: &str) -> Result<Utf8PathBuf, BuildError> {
        let hash = Hash32::hash(data);
        let hash = hash.to_hex();

        let path_temp = Utf8Path::new(".cache/hash").join(&hash);
        let path_dist = Utf8Path::new("dist/hash").join(&hash).with_extension(ext);
        let path_root = Utf8Path::new("/hash/").join(&hash).with_extension(ext);

        if !path_temp.exists() {
            fs::create_dir_all(".cache/hash")?;
            fs::write(&path_temp, data)?;
        }

        let dir = path_dist.parent().unwrap_or(&path_dist);
        fs::create_dir_all(dir)?;

        if path_dist.exists() {
            fs::remove_file(&path_dist)?;
        }

        fs::copy(&path_temp, &path_dist)?;

        Ok(path_root)
    }

    /// Registers a new entry in the global Import Map.
    ///
    /// This tells the browser how to resolve a specific module specifier.
    ///
    /// # Arguments
    ///
    /// * `key` - The module specifier (e.g., "react", "my-lib").
    /// * `value` - The URL to the module (e.g., "/hash/1234.js", "`https://cdn.example.com/lib.js`").
    pub fn register(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.imports.register(key, value);
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

/// Metadata about a file in the project structure.
///
/// This struct is typically used to provide context about where a file is located
/// relative to the project root or specific content areas.
#[derive(Debug)]
pub struct FileMetadata {
    /// The full path to the file.
    pub file: Utf8PathBuf,
    /// The "area" or base directory this file belongs to (e.g., "content", "static").
    pub area: Utf8PathBuf,
    /// Git information about the file (if available).
    pub info: Option<crate::git::GitInfo>,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_importmap() {
        let mut map = ImportMap::new();
        map.register("svelte", "/_app/svelte.js");
        assert_eq!(
            map.to_html().unwrap(),
            r#"<script type="importmap">{"imports":{"svelte":"/_app/svelte.js"}}</script>"#
        );
    }

    #[test]
    fn test_default_importmap() {
        let map = ImportMap::default();
        assert!(map.imports.is_empty());
    }

    #[test]
    fn test_merge() {
        let mut map1 = ImportMap::new();
        map1.register("a", "path/a");
        map1.register("b", "path/b");

        let mut map2 = ImportMap::new();
        map2.register("b", "path/b2");
        map2.register("c", "path/c");

        map1.merge(map2);

        // Access inner imports is not possible directly as it's private, but we can check json output
        let json = map1.to_json().unwrap();
        assert!(json.contains(r#""a":"path/a""#));
        assert!(json.contains(r#""b":"path/b2""#));
        assert!(json.contains(r#""c":"path/c""#));
    }
}
