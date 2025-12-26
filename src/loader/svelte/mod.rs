use std::{
    io::Write,
    process::{Command, Stdio},
    sync::{Arc, LazyLock},
};

use camino::Utf8Path;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    Hash32, SiteConfig,
    error::HauchiwaError,
    loader::{JS, glob::GlobRegistryTask},
    task::Handle,
};

type Prerender<P> = Arc<dyn Fn(&P) -> anyhow::Result<String> + Send + Sync>;

static RUNTIME: LazyLock<anyhow::Result<String>> = LazyLock::new(compile_svelte_runtime);

#[derive(Clone)]
pub struct Svelte<P = ()>
where
    P: serde::Serialize,
{
    /// Function that renders the component to an HTML string given props.
    pub html: Prerender<P>,

    /// Path to a JavaScript file that bootstraps client-side hydration. Written
    /// to disk during the build and referenced in the rendered output.
    pub init: JS,

    /// Path to the runtime file that provides the necessary functions for the component.
    pub rt: JS,
}

impl<G> SiteConfig<G>
where
    G: Send + Sync + 'static,
{
    pub fn build_svelte<P>(
        &mut self,
        glob_entry: &'static str,
        glob_watch: &'static str,
    ) -> Result<Handle<super::Registry<Svelte<P>>>, HauchiwaError>
    where
        P: Clone + DeserializeOwned + Serialize + 'static,
    {
        Ok(self.add_task_opaque(GlobRegistryTask::new(
            vec![glob_entry],
            vec![glob_watch],
            move |_, rt, file| {
                let svelte = RUNTIME.as_deref().unwrap();
                let svelte = rt.store(svelte.as_bytes(), "js")?;

                // If we use import maps, "svelte" in the browser needs to point
                // to our runtime file.
                rt.register("svelte", svelte.as_str());
                rt.register("svelte/internal/client", svelte.as_str());
                rt.register("svelte/internal/disclose-version", svelte.as_str());

                let server = compile_svelte_server(&file.path)?;
                let anchor = Hash32::hash(&server);
                let client = compile_svelte_init(&file.path, anchor)?;
                // let hash = Hash32::hash(&client);

                let html = Arc::new({
                    let anchor = anchor.to_hex();

                    move |props: &P| {
                        let json = serde_json::to_string(props)?;
                        let html = run_ssr(&server, &json)?;
                        let html =
                            format!("<div class='_{anchor}' data-props='{json}'>{html}</div>");
                        Ok(html)
                    }
                });

                let init = rt.store(client.as_bytes(), "js")?;
                let init = JS { path: init };

                Ok((
                    file.path,
                    Svelte::<P> {
                        html,
                        init,
                        rt: JS { path: svelte },
                    },
                ))
            },
        )?))
    }
}

fn compile_svelte_server(file: &Utf8Path) -> anyhow::Result<String> {
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
        let stdin = child
            .stdin
            .as_mut()
            .ok_or(anyhow::anyhow!("stdin not piped"))?;
        stdin.write_all(SERVER)?;
        stdin.flush()?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("Deno bundler failed:\n{stderr}"))?
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn run_ssr(server: &str, props: &str) -> anyhow::Result<String> {
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
        let stdin = child
            .stdin
            .as_mut()
            .ok_or(anyhow::anyhow!("stdin not piped"))?;
        stdin.write_all(SSR.replace("__PLACEHOLDER__", server).as_bytes())?;
        stdin.flush()?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("Deno SSR failed:\n{stderr}"))?
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn compile_svelte_init(file: &Utf8Path, hash_class: Hash32) -> anyhow::Result<String> {
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
        let stdin = child
            .stdin
            .as_mut()
            .ok_or(anyhow::anyhow!("stdin not piped"))?;
        stdin.write_all(INIT)?;
        stdin.flush()?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("Deno bundler failed:\n{stderr}"))?
    }

    Ok(String::from_utf8(output.stdout)?)
}

pub fn compile_svelte_runtime() -> anyhow::Result<String> {
    const RT: &[u8] = include_bytes!("./rt.ts");

    // Run Deno to generate the code
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
        let stdin = child
            .stdin
            .as_mut()
            .ok_or(anyhow::anyhow!("stdin not piped"))?;
        stdin.write_all(RT)?;
        stdin.flush()?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!(
            "Failed to bundle Svelte runtime:\n{stderr}"
        ))?
    }

    Ok(String::from_utf8(output.stdout)?)
}
