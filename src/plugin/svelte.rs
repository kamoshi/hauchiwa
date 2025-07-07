use std::{
    io::Write,
    process::{Command, Stdio},
};

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};

use crate::{
    Hash32,
    plugin::{Loadable, generic::LoaderGenericMultifile},
};

pub struct Svelte {
    pub html: String,
    pub init: Utf8PathBuf,
}

pub(crate) fn new_loader_svelte(path_base: &'static str, path_glob: &'static str) -> impl Loadable {
    LoaderGenericMultifile::new(
        path_base,
        path_glob,
        |path| {
            let hash = Hash32::hash(path.as_str());
            let html = compile_svelte_html(path, hash);
            let init = compile_svelte_init(path, hash);

            let mut hasher = Sha256::new();
            hasher.update(&html);
            hasher.update(&init);
            let hash: Hash32 = hasher.finalize().into();

            (hash, (html, init))
        },
        |rt, (html, init)| {
            let init = rt.store(init.as_bytes(), "js").unwrap();

            Svelte { html, init }
        },
    )
}

fn compile_svelte_html(file: &Utf8Path, hash_class: Hash32) -> String {
    const SSR: &str = r#"
        import { build } from "npm:esbuild";
        import svelte from "npm:esbuild-svelte";

        const file = Deno.args[0];
        const hash = Deno.args[1];

        const ssr = await build({
            entryPoints: [file],
            bundle: true,
            format: "esm",
            platform: "node",
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

        const { default: Comp } = await import(
            "data:text/javascript," + encodeURIComponent(ssr.outputFiles[0].text)
        );

        const data = { out: "" };
        Comp(data);

        const html = new TextEncoder().encode(`<div class="_${hash}">${data.out}</div>`);
        await Deno.stdout.write(html);
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
        .arg(hash_class.to_hex())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.as_mut().expect("stdin not piped");
        stdin.write_all(SSR.as_bytes()).unwrap();
    } // drop closes the pipe

    let output = child
        .wait_with_output()
        .expect("failed to read Deno output");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Deno bundler failed:\n{stderr}");
    }

    // println!("{}", String::from_utf8_lossy(&output.stdout));
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
                hydrate(App, { target });
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
    } // drop closes the pipe

    let output = child
        .wait_with_output()
        .expect("failed to read Deno output");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Deno bundler failed:\n{stderr}");
    }

    // println!("{}", String::from_utf8_lossy(&output.stdout));
    String::from_utf8(output.stdout).unwrap()
}
