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
    engine::HandleF,
    error::HauchiwaError,
    loader::{GlobBundle, Script},
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

    #[error("Failed to parse Deno output: {0}")]
    ParseOutput(String),
}

struct MappedJs {
    code: String,
    map: Vec<u8>,
}

// Update the Prerender type alias to use the specific error
type Prerender<P> = Arc<dyn Fn(&P) -> Result<String, SvelteError> + Send + Sync>;

// The LazyLock now holds a specific Result type.
static RUNTIME: LazyLock<Result<MappedJs, SvelteError>> = LazyLock::new(compile_svelte_runtime);

/// Represents a compiled Svelte component.
///
/// This struct allows you to:
/// 1. Server-side render (SSR) the component into HTML string using the `prerender` closure.
/// 2. Client-side hydrate the component using the scripts in `hydration` and `runtime`.
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

/// A builder for configuring the Svelte loader task.
pub struct SvelteLoader<'a, G, P>
where
    G: Send + Sync,
    P: Clone + DeserializeOwned + Serialize + 'static,
{
    blueprint: &'a mut Blueprint<G>,
    entry_globs: Vec<String>,
    watch_globs: Vec<String>,
    _phantom: std::marker::PhantomData<P>,
}

impl<'a, G, P> SvelteLoader<'a, G, P>
where
    G: Send + Sync + 'static,
    P: Clone + DeserializeOwned + Serialize + 'static,
{
    fn new(blueprint: &'a mut Blueprint<G>) -> Self {
        Self {
            blueprint,
            entry_globs: Vec::new(),
            watch_globs: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Adds a glob pattern for the entry components (e.g., "components/Button.svelte").
    pub fn entry(mut self, glob: impl Into<String>) -> Self {
        self.entry_globs.push(glob.into());
        self
    }

    /// Adds a glob pattern for files to watch (e.g., "components/**/*.svelte").
    ///
    /// If never called, defaults to watching the entry globs.
    pub fn watch(mut self, glob: impl Into<String>) -> Self {
        self.watch_globs.push(glob.into());
        self
    }

    /// Registers the task with the Blueprint.
    pub fn register(self) -> Result<HandleF<Svelte<P>>, HauchiwaError> {
        let watch_globs = if self.watch_globs.is_empty() {
            self.entry_globs.clone()
        } else {
            self.watch_globs
        };

        let task = GlobBundle::new(self.entry_globs, watch_globs, move |_, store, input| {
            let runtime = match RUNTIME.as_ref() {
                Ok(runtime) => {
                    let srcmap = store.save(&runtime.map, "js.map")?;
                    let script = format!("{}\n//# sourceMappingURL={}", runtime.code, srcmap);
                    store.save(script.as_bytes(), "js")?
                }
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
            let client = {
                let client = compile_svelte_init(&input.path, anchor)?;
                let srcmap = store.save(&client.map, "js.map")?;
                let script = format!("{}\n//# sourceMappingURL={}", client.code, srcmap);
                store.save(script.as_bytes(), "js")?
            };

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
                anchor,
                input.path,
                Svelte::<P> {
                    prerender,
                    hydration: Script { path: client },
                    runtime: Script { path: runtime },
                },
            ))
        })?;

        Ok(self.blueprint.add_task_fine(task))
    }
}

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Starts configuring a Svelte loader task.
    ///
    /// This loader uses Deno to compile Svelte components found by the entry glob.
    /// It produces an SSR-capable script and a client-side hydration script.
    ///
    /// # Generics
    ///
    /// * `P`: The type of the properties (props) that the Svelte component accepts.
    ///   This type must be serializable and deserializable.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # let mut config = hauchiwa::Blueprint::<()>::new();
    /// #[derive(serde::Serialize, serde::Deserialize, Clone)]
    /// struct ButtonProps {
    ///     label: String,
    /// }
    ///
    /// let buttons = config.load_svelte::<ButtonProps>()
    ///     .entry("components/Button.svelte")
    ///     .watch("components/**/*.svelte")
    ///     .register()?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn load_svelte<P>(&mut self) -> SvelteLoader<'_, G, P>
    where
        P: Clone + DeserializeOwned + Serialize + 'static,
    {
        SvelteLoader::new(self)
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

fn compile_svelte_init(file: &Utf8Path, hash_class: Hash32) -> Result<MappedJs, SvelteError> {
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

    parse_deno_output(&output.stdout)
}

fn compile_svelte_runtime() -> Result<MappedJs, SvelteError> {
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

    parse_deno_output(&output.stdout)
}

fn parse_deno_output(output: &[u8]) -> Result<MappedJs, SvelteError> {
    // Header format: "CODE_LEN MAP_LEN\n"
    let header_end = output
        .iter()
        .position(|&b| b == b'\n')
        .ok_or_else(|| SvelteError::ParseOutput("Missing header newline".into()))?;

    let header_str = String::from_utf8(output[0..header_end].to_vec())?;
    let parts: Vec<&str> = header_str.split_whitespace().collect();

    if parts.len() != 2 {
        return Err(SvelteError::ParseOutput("Invalid header format".into()));
    }

    let code_len: usize = parts[0]
        .parse()
        .map_err(|_| SvelteError::ParseOutput("Invalid code length".into()))?;
    let map_len: usize = parts[1]
        .parse()
        .map_err(|_| SvelteError::ParseOutput("Invalid map length".into()))?;

    let body_start = header_end + 1;
    if output.len() < body_start + code_len + map_len {
        return Err(SvelteError::ParseOutput("Incomplete data".into()));
    }

    let code_bytes = &output[body_start..body_start + code_len];
    let map_bytes = &output[body_start + code_len..body_start + code_len + map_len];

    Ok(MappedJs {
        code: String::from_utf8(code_bytes.to_vec())?,
        map: map_bytes.to_vec(),
    })
}
