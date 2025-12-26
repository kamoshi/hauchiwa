use std::process::{Command, Stdio};

use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

use crate::{SiteConfig, error::HauchiwaError, loader::GlobRegistryTask, task::Handle};

/// Errors that can occur when compiling JavaScript files.
#[derive(Debug, Error)]
pub enum ScriptError {
    /// An I/O error occurred during process execution.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The Esbuild process returned a non-zero exit code.
    #[error("Esbuild execution failed: {0}")]
    Esbuild(String),

    /// Failed to parse Esbuild output as UTF-8.
    #[error("UTF-8 conversion error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// An internal build error (e.g., failed to store the artifact).
    #[error("Build error: {0}")]
    Build(#[from] crate::error::BuildError),
}

/// Represents a compiled JavaScript module.
#[derive(Clone)]
pub struct Script {
    /// The path to the compiled JavaScript file (e.g., hashed path).
    pub path: Utf8PathBuf,
}

impl<G> SiteConfig<G>
where
    G: Send + Sync + 'static,
{
    /// Compiles JavaScript files using Esbuild.
    ///
    /// This loader finds files matching `glob_entry`, bundles and minifies them
    /// using the `esbuild` command-line tool, and stores the resulting artifacts.
    ///
    /// **Note:** This loader requires the `esbuild` binary to be available in the system PATH.
    ///
    /// # Arguments
    ///
    /// * `glob_entry`: Glob pattern for the entry points (e.g., "src/main.ts").
    /// * `glob_watch`: Glob pattern for files to watch for changes (often broader, e.g., "src/**/*.ts").
    ///
    /// # Returns
    ///
    /// A handle to a registry mapping original file paths to `JS` objects.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Compile main.ts using esbuild, watching all ts files in the scripts directory.
    /// let scripts = config.load_js("scripts/main.ts", "scripts/**/*.ts")?;
    /// ```
    pub fn load_js(
        &mut self,
        glob_entry: &'static str,
        glob_watch: &'static str,
    ) -> Result<Handle<super::Registry<Script>>, HauchiwaError> {
        Ok(self.add_task_opaque(GlobRegistryTask::new(
            vec![glob_entry],
            vec![glob_watch],
            move |_, rt, file| {
                let data = compile_esbuild(&file.path)?;
                let path = rt.store(&data, "js").map_err(ScriptError::Build)?;

                Ok((file.path, Script { path }))
            },
        )?))
    }
}

fn compile_esbuild(file: &Utf8Path) -> Result<Vec<u8>, ScriptError> {
    let output = Command::new("esbuild")
        .arg(file.as_str())
        .arg("--format=esm")
        .arg("--bundle")
        .arg("--minify")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()?;

    if !output.status.success() {
        return Err(ScriptError::Esbuild(String::from_utf8(output.stdout)?));
    }

    Ok(output.stdout)
}
