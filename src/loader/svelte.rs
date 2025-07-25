use std::{
    io::Write,
    process::{Command, Stdio},
};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{Hash32, Loader, loader::generic::LoaderGenericMultifile};

type Prerender<P> = Box<dyn Fn(&P) -> anyhow::Result<String> + Send + Sync>;

/// Represents a pre-rendered Svelte component with client-side hydration.
///
/// The `html` field is a closure that accepts a serializable props struct `P`
/// and returns the rendered HTML string using the compiled Svelte SSR bundle.
/// The `init` field is a path to a browser-ready ES module responsible for
/// hydrating the client-side component, scoped by hash.
///
/// This struct is constructed by the `glob_svelte` loader, which bundles both
/// the SSR and client init code from `.svelte` sources using Deno and esbuild.
pub struct Svelte<P = ()>
where
    P: serde::Serialize,
{
    /// Function that renders the component to an HTML string given props.
    pub html: Prerender<P>,

    /// Path to a JavaScript file that bootstraps client-side hydration. Written
    /// to disk during the build and referenced in the rendered output.
    pub init: Utf8PathBuf,
}

/// Constructs a loader that processes Svelte components into pre-renderable assets.
///
/// This function takes a base path and a glob pattern to locate `.svelte` files.
/// For each match, it:
///
/// - Compiles the file into a server-side rendering module (via Deno + esbuild).
/// - Compiles a corresponding client-side hydration script with a unique class hash.
/// - Stores both under a content hash derived from their output.
///
/// The returned loader emits a [`Svelte<P>`] object, where `P` is the expected
/// props type. You can render it to HTML via the `html` closure and emit the
/// associated `init` JS during the build.
///
/// ### Requirements
///
/// - Deno must be installed and in `$PATH`.
/// - Props must be serializable to JSON (via [`serde::Serialize`]).
///
/// ### Example
///
/// ```rust
/// use hauchiwa::{Context, TaskResult, Page, loader::{Svelte, glob_svelte}};
///
/// type MyProps = ();
///
/// // loader
/// let loader = glob_svelte::<MyProps>("src/components", "**/*.svelte");
///
/// // task
/// fn task(ctx: Context) -> TaskResult<Vec<Page>> {
///     let Svelte { html, init } = ctx.get::<Svelte>("App.svelte")?;
///
///     // prerender HTML component
///     let html = html(&())?;
///
///     Ok(vec![
///         Page::text("index.html".into(), format!("{html} <script type='module' src='{init}'></script>"))
///     ])
/// }
/// ```
///
/// The resulting items can be accessed through the build context and rendered
/// as [`Svelte<MyProps>`] during templating.
pub fn glob_svelte<P>(path_base: &'static str, path_glob: &'static str) -> Loader
where
    P: serde::Serialize + 'static,
{
    Loader::with(move |_| {
        LoaderGenericMultifile::new(
            path_base,
            path_glob,
            |path| {
                let server = compile_svelte_server(path)?;
                let anchor = Hash32::hash(&server);
                let client = compile_svelte_init(path, anchor)?;
                let hash = Hash32::hash(&client);

                let html = Box::new({
                    let anchor = anchor.to_hex();

                    move |props: &P| {
                        let json = serde_json::to_string(props)?;
                        let html = run_ssr(&server, &json)?;
                        let html =
                            format!("<div class='_{anchor}' data-props='{json}'>{html}</div>");
                        Ok(html)
                    }
                });

                Ok((hash, (html, client)))
            },
            |rt, (html, init)| {
                let init = rt.store(init.as_bytes(), "js")?;

                Ok(Svelte { html, init })
            },
        )
    })
}

fn compile_svelte_server(file: &Utf8Path) -> anyhow::Result<String> {
    const JS: &str = r#"
        import { build } from "npm:esbuild@0.25.6";
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
        const data = {{ out: "" }};
        SSR(data, props);

        const html = new TextEncoder().encode(data.out);
        await Deno.stdout.write(html);
        await Deno.stdout.close();
    "#
    );

    let mut child = Command::new("deno")
        .arg("run")
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
        import { build } from "npm:esbuild@0.25.6";
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
