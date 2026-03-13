//! # JavaScript/TypeScript bundling pipeline
//!
//! Blazingly fast compilation and bundling natively in Rust using [Rolldown](https://rolldown.rs/).
//!
//! This module integrates the `rolldown` crate, allowing you to treat JavaScript
//! and TypeScript files as first-class citizens in your build graph. It
//! automatically handles transpilation, dependency resolution, minification,
//! and content-hashing natively without requiring external binaries on the system PATH.
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
//! use hauchiwa::loader::js::Script;
//!
//! fn configure(config: &mut Blueprint<()>) -> anyhow::Result<Many<Script>> {
//!     // Compile main.ts -> dist/hash/js/main.[hash].js
//!     let app = config.load_rolldown()
//!         .entry("src/client/main.ts")
//!         .watch("src/client/**/*.ts") // Rebuild when any client file changes
//!         .bundle(true)
//!         .minify(true)
//!         .register()?;
//!
//!     Ok(app)
//! }
//! ```

use camino::Utf8Path;
use rolldown::{BundlerOptions, CodeSplittingMode, InputItem, RawMinifyOptions};
use thiserror::Error;

use crate::core::Hash32;
use crate::{Blueprint, engine::Many, error::HauchiwaError, loader::GlobBundle};

/// Errors that can occur when compiling JavaScript files.
#[derive(Debug, Error)]
pub enum ScriptError {
    /// An I/O error occurred during processing.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The Rolldown compilation failed.
    #[error("Rolldown execution failed: {0}")]
    Rolldown(String),

    /// Failed to parse output as UTF-8.
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
    watch_globs: Vec<String>,
    bundle: bool,
    minify: bool,
}

impl<'a, G> ScriptLoader<'a, G>
where
    G: Send + Sync + 'static,
{
    pub(crate) fn new(blueprint: &'a mut Blueprint<G>) -> Self {
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
    pub fn register(self) -> Result<Many<super::Script>, HauchiwaError> {
        let watch_globs = if self.watch_globs.is_empty() {
            self.entry_globs.clone()
        } else {
            self.watch_globs
        };

        let bundle = self.bundle;
        let minify = self.minify;

        let task = GlobBundle::new(self.entry_globs, watch_globs, move |_, store, input| {
            let data = compile_rolldown(&input.path, bundle, minify)?;
            let hash = Hash32::hash(&data);
            let path = store.save(&data, "js").map_err(ScriptError::Build)?;

            Ok((hash, input.path, super::Script { path }))
        })?;

        Ok(self.blueprint.add_task_fine(task))
    }
}

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Starts configuring a JavaScript loader task using Rolldown.
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
    pub fn load_rolldown(&mut self) -> ScriptLoader<'_, G> {
        ScriptLoader::new(self)
    }
}

fn compile_rolldown(file: &Utf8Path, _bundle: bool, minify: bool) -> Result<Vec<u8>, ScriptError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let options = BundlerOptions {
            // Define the entry point for the bundler
            input: Some(vec![InputItem {
                name: Some(file.to_string()),
                import: file.to_string(),
            }]),
            minify: Some(RawMinifyOptions::Bool(minify)),
            code_splitting: Some(CodeSplittingMode::Bool(false)),

            ..Default::default()
        };

        // Create the bundler and generate the output
        let mut bundler =
            rolldown::Bundler::new(options).map_err(|e| ScriptError::Rolldown(e.to_string()))?;
        let output = bundler
            .generate()
            .await
            .map_err(|e| ScriptError::Rolldown(e.to_string()))?;

        // Extract the bundled JavaScript code from the generated assets
        if let Some(chunk) = output.assets.into_iter().next() {
            return Ok(chunk.content_as_bytes().to_vec());
        }

        Err(ScriptError::Rolldown("No output chunks generated".into()))
    })
}
