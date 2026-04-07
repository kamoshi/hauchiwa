//! # JavaScript/TypeScript bundling pipeline
//!
//! Blazingly fast compilation and bundling using [esbuild](https://esbuild.github.io/).
//!
//! This module acts as a bridge to `esbuild`, allowing you to treat JavaScript
//! and TypeScript files as first-class citizens in your build graph. It
//! automatically handles transpilation, dependency resolution, minification,
//! and content-hashing.
//!
//! **Note**: Requires the `esbuild` binary to be available in your system PATH.
//!
//! ## Capabilities
//!
//! * **TypeScript**: Native support for `.ts` and `.tsx` files without extra config.
//! * **Bundling**: Recursively resolves `import`s to produce a single self-contained file.
//! * **Optimization**: Minifies code for production by default.
//! * **Cache Busting**: Output files are hashed for immutable caching.
//!
//! ## Usage
//!
//! Register the loader to generate a handle containing the public path to your script.
//!
//! ```rust,no_run
//! use hauchiwa::{Blueprint, Many};
//! use hauchiwa::loader::Script;
//!
//! fn configure(config: &mut Blueprint<()>) -> Result<Many<Script>, hauchiwa::error::HauchiwaError> {
//!     let app = config.load_esbuild()
//!         .entry("src/client/main.ts")?
//!         .watch("src/client/**/*.ts")?
//!         .register();
//!
//!     Ok(app)
//! }
//! ```
use std::io::Write;
use std::process::{Command, Stdio};

use camino::Utf8Path;
use glob::Pattern;
use thiserror::Error;

use crate::core::Hash32;
use crate::{Blueprint, engine::Many, error::HauchiwaError, loader::GlobBundle};

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

/// A builder for configuring the Script loader task.
pub struct ScriptLoader<'a, G>
where
    G: Send + Sync,
{
    blueprint: &'a mut Blueprint<G>,
    entry_globs: Vec<String>,
    entry_patterns: Vec<Pattern>,
    watch_globs: Vec<Pattern>,
    bundle: bool,
    minify: bool,
    externals: Vec<String>,
}

impl<'a, G> ScriptLoader<'a, G>
where
    G: Send + Sync + 'static,
{
    fn new(blueprint: &'a mut Blueprint<G>) -> Self {
        Self {
            blueprint,
            entry_globs: Vec::new(),
            entry_patterns: Vec::new(),
            watch_globs: Vec::new(),
            bundle: true,
            minify: true,
            externals: Vec::new(),
        }
    }

    /// Adds a glob pattern for the entry points (e.g., "src/main.ts").
    pub fn entry(mut self, glob: impl Into<String>) -> Result<Self, HauchiwaError> {
        let glob = glob.into();
        let pattern = Pattern::new(&glob)?;
        self.entry_globs.push(glob);
        self.entry_patterns.push(pattern);
        Ok(self)
    }

    /// Adds a glob pattern for files to watch for changes (often broader, e.g., "src/**/*.ts").
    ///
    /// If never called, defaults to watching the entry globs.
    pub fn watch(mut self, glob: impl Into<String>) -> Result<Self, HauchiwaError> {
        let glob = glob.into();
        let pattern = Pattern::new(&glob)?;
        self.watch_globs.push(pattern);
        Ok(self)
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

    /// Marks a package as external.
    ///
    /// The package is bundled separately as a content-addressed file and
    /// registered in the import map. The main entry point is compiled with
    /// `--external:<package>` so the browser resolves it via the import map
    /// at runtime.
    pub fn external(mut self, package: impl Into<String>) -> Self {
        self.externals.push(package.into());
        self
    }

    /// Finalizes configuration and registers the task with the Blueprint.
    ///
    /// Returns a [`Many<Script>`] handle that resolves to one compiled output
    /// per matched entry file.
    pub fn register(self) -> Many<super::Script> {
        let watch_globs = if self.watch_globs.is_empty() {
            self.entry_patterns
        } else {
            self.watch_globs
        };

        let bundle = self.bundle;
        let minify = self.minify;
        let externals = self.externals;

        let task =
            GlobBundle::new(self.entry_globs, watch_globs, move |_, store, input| {
                for package in &externals {
                    let data = bundle_package(package, minify)?;
                    let path = store.save(&data, "js").map_err(ScriptError::Build)?;
                    store.register(package.as_str(), path.as_str());
                }

                let data = compile_esbuild(&input.path, bundle, minify, &externals)?;
                let hash = Hash32::hash(&data);
                let path = store.save(&data, "js").map_err(ScriptError::Build)?;

                Ok((hash, input.path, super::Script { path }))
            });

        self.blueprint.add_task_fine(task)
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
    /// config.load_esbuild()
    ///     .entry("scripts/main.ts")?
    ///     .watch("scripts/**/*.ts")?
    ///     .register();
    /// # Ok::<(), hauchiwa::error::HauchiwaError>(())
    /// ```
    pub fn load_esbuild(&mut self) -> ScriptLoader<'_, G> {
        ScriptLoader::new(self)
    }
}

fn compile_esbuild(
    file: &Utf8Path,
    bundle: bool,
    minify: bool,
    externals: &[String],
) -> Result<Vec<u8>, ScriptError> {
    let mut cmd = Command::new("esbuild");
    cmd.arg(file.as_str()).arg("--format=esm");

    if bundle {
        cmd.arg("--bundle");
    }

    if minify {
        cmd.arg("--minify");
    }

    for package in externals {
        cmd.arg(format!("--external:{package}"));
    }

    let output = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).output()?;

    if !output.status.success() {
        return Err(ScriptError::Esbuild(String::from_utf8(output.stderr)?));
    }

    Ok(output.stdout)
}

fn bundle_package(package: &str, minify: bool) -> Result<Vec<u8>, ScriptError> {
    let stdin_content = format!("export * from '{package}'");

    let mut cmd = Command::new("esbuild");
    cmd.arg("--bundle")
        .arg("--format=esm")
        .arg("--platform=browser")
        .arg("--loader=js");

    if minify {
        cmd.arg("--minify");
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin was not piped"))?
        .write_all(stdin_content.as_bytes())?;

    let output = child.wait_with_output()?;

    if !output.status.success() {
        return Err(ScriptError::Esbuild(String::from_utf8(output.stderr)?));
    }

    Ok(output.stdout)
}
