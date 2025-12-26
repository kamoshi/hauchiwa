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
    const JS: &str = r#"
        import { build } from "npm:esbuild@0.25.11";
        import svelte from "npm:esbuild-svelte@0.9.3";

        const file = Deno.args[0];

        const ssr = await build({
            entryPoints: [file],
            format: "esm",
            platform: "node",
            minify: true,
            bundle: true,
            write: false,
            mainFields: ["svelte", "module", "main"],
            conditions: ["svelte"],
            plugins: [
                svelte({
                    compilerOptions: { generate: "server" },
                    css: false,
                    emitCss: false,
                }),
            ],
        });

        const text = encodeURIComponent(ssr.outputFiles[0].text);
        const js = new TextEncoder().encode(text);
        await Deno.stdout.write(js);
        await Deno.stdout.close();
    "#;

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
        stdin.write_all(JS.as_bytes())?;
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
    let js = format!(
        r#"
        const json = Deno.args[0];
        const props = JSON.parse(json);

        const {{ default: SSR }} = await import("data:text/javascript,{server}");

        let output = null;

        if (!output) {{
            try {{
                const data = {{ out: [] }};
                SSR(data, props);
                output = data.out.join();
            }} catch {{ }}
        }}

        if (!output) {{
            try {{
                const data = {{ out: "" }};
                SSR(data, props);
                output = data.out;
            }} catch {{ }}
        }}

        if (!output) {{
            throw "Failed to produce prerendered component, are you using svelte 5?";
        }}

        const html = new TextEncoder().encode(output);
        await Deno.stdout.write(html);
        await Deno.stdout.close();
    "#
    );

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
        stdin.write_all(js.as_bytes())?;
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
    const JS: &str = r#"
        import * as path from "node:path";
        import { build } from "npm:esbuild@0.25.11";
        import svelte from "npm:esbuild-svelte@0.9.3";

        const file = Deno.args[0];
        const hash = Deno.args[1];

        const stub = `
            import { hydrate } from "svelte";
            import App from ${JSON.stringify(file)};

            const query = document.querySelectorAll('._${hash}');
            for (const target of query) {
                const attrs = target.getAttribute('data-props');
                const props = JSON.parse(attrs) ?? {};
                hydrate(App, { target, props });
            }
        `;

        const ssr = await build({
            stdin: {
                contents: stub,
                resolveDir: path.dirname(path.resolve(file)),
                sourcefile: "__virtual.ts",
                loader: "ts",
            },
            platform: "browser",
            format: "esm",
            bundle: true,
            minify: true,
            write: false,
            mainFields: ["svelte", "browser", "module", "main"],
            conditions: ["svelte", "browser"],
            external: ["svelte"],
            plugins: [
                svelte({
                    compilerOptions: {
                        css: "external",
                    },
                }),
            ],
        });

        const js = new TextEncoder().encode(ssr.outputFiles[0].text);
        await Deno.stdout.write(js);
        await Deno.stdout.close();
    "#;

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
        stdin.write_all(JS.as_bytes())?;
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
    const JS: &str = r#"
        import { build } from "npm:esbuild@0.25.11";
        // Ensure this matches the version used in other functions or relies on the same resolution

        // Create a virtual entry point that re-exports Svelte features
        // 'hydrate' is the critical one used in compile_svelte_init
        const stub = `
            // 1. The Public API (mount, flushSync, etc.)
            export * from "svelte";

            // 2. The Svelte 5 Engine (CRITICAL for your error)
            // The compiled code imports 'template_effect', 'append', etc. from here.
            export * from "svelte/internal/client";

            // 3. Side-effects (Version disclosure)
            // This file just sets window.__svelte.v = '5.x'.
            // We import it for side-effects so it gets bundled.
            import "svelte/internal/disclose-version";
        `;

        const bundle = await build({
            stdin: {
                contents: stub,
                resolveDir: Deno.cwd(), // Resolve from current working directory
                loader: "ts",
            },
            platform: "browser",
            format: "esm",
            bundle: true,      // Bundle Svelte into this file
            minify: true,
            write: false,
            // Ensure we use the exact same conditions as the component loader
            mainFields: ["svelte", "browser", "module", "main"],
            conditions: ["svelte", "browser"],
        });

        const js = new TextEncoder().encode(bundle.outputFiles[0].text);
        await Deno.stdout.write(js);
        await Deno.stdout.close();
    "#;

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
        stdin.write_all(JS.as_bytes())?;
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
