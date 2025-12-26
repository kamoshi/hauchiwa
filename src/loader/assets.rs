use crate::{
    Globals, SiteConfig,
    error::HauchiwaError,
    loader::{File, Runtime, glob::GlobRegistryTask},
    task::Handle,
};

/// Adds a task to the site configuration that finds files matching a glob
/// pattern, processes each file using a provided callback, and collects the
/// results into a `Registry`.
///
/// This function creates and registers a `GlobRegistryTask`. When the build
/// graph is executed, this task will:
/// 1.  Find all files on the filesystem that match the `path_glob`.
/// 2.  For each file found, it reads its content as raw bytes (`Vec<u8>`).
/// 3.  It then invokes the provided `callback` for each file, passing in the
///     global context (`&Globals<G>`) and a `File<Vec<u8>>` struct.
/// 4.  The `callback` processes the file and returns a value `R`.
/// 5.  The task collects all results and returns a `Registry<R>`, which is a
///     map from the original file path (`Utf8PathBuf`) to the corresponding `R` value.
///
/// This task will also be marked as dirty (triggering a rebuild) if any file
/// matching the `path_glob` is modified.
///
/// # Parameters
///
/// * `site_config`: The mutable `SiteConfig` to which the new task will be added.
/// * `path_glob`: A glob pattern (e.g., `"static/**/*"`) used to find files. This
///   pattern is used for both the initial file discovery and for watching for changes.
/// * `callback`: A closure that defines the processing for each file. It receives
///   the `&Globals<G>` and a `File<Vec<u8>>` (containing the file's path and
///   raw byte content) and must return an `anyhow::Result<R>`.
///
/// # Generics
///
/// * `G`: The type of the global data.
/// * `R`: The return type of the `callback` for a single file. This is the
///   value that will be stored in the `Registry`.
///
/// # Returns
///
/// Returns a `Handle<super::Registry<R>>`, which is a typed reference to the
/// task's output in the build graph. The output will be the `Registry`
/// containing all processed file results.
pub fn glob_assets<G, R>(
    config: &mut SiteConfig<G>,
    path_glob: &'static str,
    callback: impl Fn(&Globals<G>, &mut Runtime, File<Vec<u8>>) -> anyhow::Result<R>
    + Send
    + Sync
    + 'static,
) -> Result<Handle<super::Registry<R>>, HauchiwaError>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    Ok(config.add_task_opaque(GlobRegistryTask::new(
        vec![path_glob],
        vec![path_glob],
        move |ctx, rt, file| {
            let path = file.path.clone();
            let res = callback(ctx.globals, rt, file)?;

            Ok((path, res))
        },
    )?))
}
