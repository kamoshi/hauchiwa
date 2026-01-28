use std::process::{Command, Stdio};

use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

use crate::{Blueprint, error::HauchiwaError, graph::Handle, loader::GlobAssetsTask};

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

/// A builder for configuring the Script loader task.
pub struct ScriptLoader<'a, G>
where
    G: Send + Sync,
{
    blueprint: &'a mut Blueprint<G>,
    entry_globs: Vec<String>,
    watch_globs: Vec<String>,
    bundle: bool,
    minify: bool,
}

impl<'a, G> ScriptLoader<'a, G>
where
    G: Send + Sync + 'static,
{
    fn new(blueprint: &'a mut Blueprint<G>) -> Self {
        Self {
            blueprint,
            entry_globs: Vec::new(),
            watch_globs: Vec::new(),
            bundle: true,
            minify: true,
        }
    }

    /// Adds a glob pattern for the entry points (e.g., "src/main.ts").
    pub fn entry(mut self, glob: impl Into<String>) -> Self {
        self.entry_globs.push(glob.into());
        self
    }

    /// Adds a glob pattern for files to watch for changes (often broader, e.g., "src/**/*.ts").
    ///
    /// If never called, defaults to watching the entry globs.
    pub fn watch(mut self, glob: impl Into<String>) -> Self {
        self.watch_globs.push(glob.into());
        self
    }

    /// Toggles bundling dependencies. Defaults to `true`.
    pub fn bundle(mut self, bundle: bool) -> Self {
        self.bundle = bundle;
        self
    }

    /// Toggles minification. Defaults to `true`.
    pub fn minify(mut self, minify: bool) -> Self {
        self.minify = minify;
        self
    }

    /// Registers the task with the Blueprint.
    pub fn register(self) -> Result<Handle<super::Assets<Script>>, HauchiwaError> {
        let watch_globs = if self.watch_globs.is_empty() {
            self.entry_globs.clone()
        } else {
            self.watch_globs
        };

        let bundle = self.bundle;
        let minify = self.minify;

        Ok(self.blueprint.add_task_opaque(GlobAssetsTask::new(
            self.entry_globs,
            watch_globs,
            move |_, store, input| {
                let data = compile_esbuild(&input.path, bundle, minify)?;
                let path = store.save(&data, "js").map_err(ScriptError::Build)?;

                Ok((input.path, Script { path }))
            },
        )?))
    }
}

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Starts configuring a JavaScript loader task.
    ///
    /// This loader uses `esbuild` to compile, bundle, and minify JavaScript/TypeScript files.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # let mut config = hauchiwa::Blueprint::<()>::new();
    /// config.load_js()
    ///     .entry("scripts/main.ts")
    ///     .watch("scripts/**/*.ts")
    ///     .bundle(true)
    ///     .minify(true)
    ///     .register()?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn load_js(&mut self) -> ScriptLoader<'_, G> {
        ScriptLoader::new(self)
    }
}

fn compile_esbuild(file: &Utf8Path, bundle: bool, minify: bool) -> Result<Vec<u8>, ScriptError> {
    let mut cmd = Command::new("esbuild");
    cmd.arg(file.as_str()).arg("--format=esm");

    if bundle {
        cmd.arg("--bundle");
    }

    if minify {
        cmd.arg("--minify");
    }

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()?;

    if !output.status.success() {
        return Err(ScriptError::Esbuild(String::from_utf8(output.stdout)?));
    }

    Ok(output.stdout)
}
