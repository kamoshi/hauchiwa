use std::{
    io::Write,
    process::{Command, Stdio},
};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{Hash32, Loader, loader::generic::LoaderGenericMultifile};

pub struct Svelte<P>
where
    P: serde::Serialize,
{
    pub html: Box<dyn Fn(&P) -> String + Send + Sync>,
    pub init: Utf8PathBuf,
}

pub fn glob_svelte<P>(path_base: &'static str, path_glob: &'static str) -> Loader
where
    P: serde::Serialize + 'static,
{
    Loader::with(move |_| {
        LoaderGenericMultifile::new(
            path_base,
            path_glob,
            |path| {
                let server = compile_svelte_server(path);
                let hash = Hash32::hash(&server);
                let client = compile_svelte_init(path, hash);

                let ssr = Box::new({
                    let hashed = hash.to_hex();

                    move |props: &P| {
                        let json = serde_json::to_string(props).unwrap();
                        let html = run_ssr(&server, &json);
                        format!(r#"<div class="_{hashed}" data-props='{json}'>{html}</div>"#,)
                    }
                });

                (hash, (ssr, client))
            },
            |rt, (html, init)| {
                let init = rt.store(init.as_bytes(), "js").unwrap();

                Svelte { html, init }
            },
        )
    })
}

fn compile_svelte_server(file: &Utf8Path) -> String {
    const SSR: &str = r#"
        import { build } from "npm:esbuild";
        import svelte from "npm:esbuild-svelte";

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
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.as_mut().expect("stdin not piped");
        stdin.write_all(SSR.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }

    let output = child
        .wait_with_output()
        .expect("failed to read Deno output");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Deno bundler failed:\n{stderr}");
    }

    String::from_utf8(output.stdout).unwrap()
}

fn run_ssr(server: &str, props: &str) -> String {
    let code = format!(
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
        .spawn()
        .expect("failed to start Deno SSR server");

    {
        let stdin = child.stdin.as_mut().expect("stdin not piped");
        stdin.write_all(code.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }

    let output = child
        .wait_with_output()
        .expect("failed to read Deno output");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Deno SSR failed:\n{stderr}");
    }

    String::from_utf8(output.stdout).unwrap()
}

fn compile_svelte_init(file: &Utf8Path, hash_class: Hash32) -> String {
    const SSR: &str = r#"
        import * as path from "node:path";
        import { build } from "npm:esbuild";
        import svelte from "npm:esbuild-svelte";

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
        .arg(file.canonicalize().unwrap())
        .arg(hash_class.to_hex())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.as_mut().expect("stdin not piped");
        stdin.write_all(SSR.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }

    let output = child
        .wait_with_output()
        .expect("failed to read Deno output");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Deno bundler failed:\n{stderr}");
    }

    String::from_utf8(output.stdout).unwrap()
}
