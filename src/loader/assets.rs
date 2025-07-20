use std::fs;

use crate::{
    Hash32, Loader,
    loader::{Runtime, generic::LoaderGeneric},
};

/// A generic loader for binary assets with custom deserialization or transformation.
///
/// Scans for files matching the glob under the given base path, computes a content
/// hash, and invokes a user-supplied function to produce a typed `T` from the raw bytes.
///
/// ### Parameters
/// - `path_base`: The base directory for resolving the glob.
/// - `path_glob`: Glob pattern (relative to `path_base`) identifying the assets.
/// - `func`: A function that receives the build `Runtime` and the raw file contents,
///   returning a `Result<T>` that will be cached and stored.
///
/// ### Example
/// ```rust
/// use hauchiwa::loader::{Runtime, glob_assets};
///
/// fn count_bytes(_: Runtime, bytes: Vec<u8>) -> anyhow::Result<usize> {
///     Ok(bytes.len())
/// }
///
/// let loader = glob_assets("src/data", "**/*.bin", count_bytes);
/// ```
///
/// ### Output
/// Produces a [`Loader`] that stores a content-hashed instance of `T` per asset.
///
/// ### Notes
/// - Files are hashed and compared by content, not path.
/// - `func` is executed lazily per file during load.
/// - `func` must be deterministic and free of side effects for reproducibility
pub fn glob_assets<T>(
    path_base: &'static str,
    path_glob: &'static str,
    func: fn(Runtime, Vec<u8>) -> anyhow::Result<T>,
) -> Loader
where
    T: Send + Sync + 'static,
{
    Loader::with(move |_| {
        LoaderGeneric::new(
            path_base,
            path_glob,
            |path| {
                let data = fs::read(path)?;
                let hash = Hash32::hash(&data);

                Ok((hash, data))
            },
            func,
        )
    })
}
