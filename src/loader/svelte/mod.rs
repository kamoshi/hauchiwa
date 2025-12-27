use std::{
    io::Write,
    process::{Command, Stdio},
    sync::{Arc, LazyLock},
};

use camino::Utf8Path;
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::{
    Blueprint, Hash32,
    error::HauchiwaError,
    graph::Handle,
    loader::{GlobAssetsTask, Script},
};

#[derive(Debug, Error)]
pub enum SvelteError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("UTF-8 conversion error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Deno execution failed: {0}")]
    Deno(String),

    #[error("Failed to capture child process stdin")]
    StdinCapture,

    #[error("Svelte runtime compilation failed: {0}")]
    Runtime(String),
}

// Update the Prerender type alias to use the specific error
type Prerender<P> = Arc<dyn Fn(&P) -> Result<String, SvelteError> + Send + Sync>;

// The LazyLock now holds a specific Result type.
static RUNTIME: LazyLock<Result<String, SvelteError>> = LazyLock::new(compile_svelte_runtime);

/// Represents a compiled Svelte component.
///
/// This struct allows you to:
/// 1. Server-side render (SSR) the component into HTML string using the `html` closure.
/// 2. Client-side hydrate the component using the scripts in `init` and `rt`.
///
/// # Generics
///
/// * `P`: The type of the component's props.
#[derive(Clone)]
pub struct Svelte<P = ()>
where
    P: serde::Serialize,
{
    /// A closure that takes props `P` and returns the rendered HTML string.
    /// This is used for Server-Side Rendering (SSR).
    pub prerender: Prerender<P>,
    /// The initialization script for this specific component (client-side hydration).
    pub hydration: Script,
    /// The shared Svelte runtime library script.
    pub runtime: Script,
}

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Compiles Svelte components for Server-Side Rendering (SSR) and client-side hydration.
    ///
    /// This loader uses Deno to compile Svelte components found by the entry glob.
    /// It produces an SSR-capable script and a client-side hydration script.
    ///
    /// The `Svelte` object returned in the registry provides an `html` function (for SSR)
    /// and handles to the client-side JavaScript assets.
    ///
    /// **Note:** This loader requires the `deno` binary to be available in the system PATH.
    ///
    /// # Generics
    ///
    /// * `P`: The type of the properties (props) that the Svelte component accepts.
    ///   This type must be serializable and deserializable.
    ///
    /// # Arguments
    ///
    /// * `glob_entry`: Glob pattern for the entry components (e.g., "components/Button.svelte").
    /// * `glob_watch`: Glob pattern for files to watch (e.g., "components/**/*.svelte").
    ///
    /// # Returns
    ///
    /// A handle to a registry mapping original file paths to `Svelte<P>` objects.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// #[derive(serde::Serialize, serde::Deserialize, Clone)]
    /// struct ButtonProps {
    ///     label: String,
    /// }
    ///
    /// let buttons = config.load_svelte::<ButtonProps>(
    ///     "components/Button.svelte",
    ///     "components/**/*.svelte"
    /// )?;
    /// ```
    pub fn load_svelte<P>(
        &mut self,
        glob_entry: &'static str,
        glob_watch: &'static str,
    ) -> Result<Handle<super::Assets<Svelte<P>>>, HauchiwaError>
    where
        P: Clone + DeserializeOwned + Serialize + 'static,
    {
        Ok(self.add_task_opaque(GlobAssetsTask::new(
            vec![glob_entry],
            vec![glob_watch],
            move |_, store, input| {
                let runtime = match RUNTIME.as_ref() {
                    Ok(runtime) => store.save(runtime.as_bytes(), "js")?,
                    Err(err) => return Err(SvelteError::Runtime(err.to_string()).into()),
                };

                // In the import map "svelte" should be registered, so that it
                // points to the runtime file.
                store.register("svelte", runtime.as_str());
                store.register("svelte/internal/client", runtime.as_str());
                store.register("svelte/internal/disclose-version", runtime.as_str());

                // Compile the SSR script
                let server = compile_svelte_server(&input.path)?;
                let anchor = Hash32::hash(&server);

                // Compile lean browser glue
                let client = compile_svelte_init(&input.path, anchor)?;
                let client = store.save(client.as_bytes(), "js")?;

                // With the compiled SSR script we can now pre-render the
                // component on demand.
                let prerender = Arc::new({
                    let anchor = anchor.to_hex();

                    move |props: &P| {
                        let json = serde_json::to_string(props)?;
                        let html = run_ssr(&server, &json)?;

                        Ok(format!(
                            "<div class='_{anchor}' data-props='{json}'>{html}</div>"
                        ))
                    }
                });

                Ok((
                    input.path,
                    Svelte::<P> {
                        prerender,
                        hydration: Script { path: client },
                        runtime: Script { path: runtime },
                    },
                ))
            },
        )?))
    }
}

fn compile_svelte_server(file: &Utf8Path) -> Result<String, SvelteError> {
    const SERVER: &[u8] = include_bytes!("./server.ts");

    let mut child = Command::new("deno")
        .arg("run")
        .arg("--quiet")
        .arg("--allow-env")
        .arg("--allow-read")
        .arg("--allow-run")
        .arg("-")
        .arg(file.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    {
        let stdin = child.stdin.as_mut().ok_or(SvelteError::StdinCapture)?;
        stdin.write_all(SERVER)?;
        stdin.flush()?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SvelteError::Deno(format!("Deno bundler failed:\n{stderr}")));
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn run_ssr(server: &str, props: &str) -> Result<String, SvelteError> {
    const SSR: &str = include_str!("./ssr.ts");

    let mut child = Command::new("deno")
        .arg("run")
        .arg("--allow-env")
        .arg("--quiet")
        .arg("-")
        .arg(props)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    {
        let stdin = child.stdin.as_mut().ok_or(SvelteError::StdinCapture)?;
        stdin.write_all(SSR.replace("__PLACEHOLDER__", server).as_bytes())?;
        stdin.flush()?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SvelteError::Deno(format!("Deno SSR failed:\n{stderr}")));
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn compile_svelte_init(file: &Utf8Path, hash_class: Hash32) -> Result<String, SvelteError> {
    const INIT: &[u8] = include_bytes!("./init.ts");

    let mut child = Command::new("deno")
        .arg("run")
        .arg("--quiet")
        .arg("--allow-env")
        .arg("--allow-read")
        .arg("--allow-run")
        .arg("-")
        .arg(file.canonicalize()?)
        .arg(hash_class.to_hex())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    {
        let stdin = child.stdin.as_mut().ok_or(SvelteError::StdinCapture)?;
        stdin.write_all(INIT)?;
        stdin.flush()?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SvelteError::Deno(format!("Deno bundler failed:\n{stderr}")));
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn compile_svelte_runtime() -> Result<String, SvelteError> {
    const RT: &[u8] = include_bytes!("./rt.ts");

    let mut child = Command::new("deno")
        .arg("run")
        .arg("--quiet")
        .arg("--allow-env")
        .arg("--allow-read")
        .arg("--allow-run")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    {
        let stdin = child.stdin.as_mut().ok_or(SvelteError::StdinCapture)?;
        stdin.write_all(RT)?;
        stdin.flush()?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SvelteError::Deno(format!(
            "Failed to bundle Svelte runtime:\n{stderr}"
        )));
    }

    Ok(String::from_utf8(output.stdout)?)
}
