use std::{
    io::Write,
    process::{Command, Stdio},
};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    SiteConfig,
    loader::{File, Runtime, glob::GlobLoaderTask},
    task::Handle,
};

#[derive(Clone)]
pub struct Svelte {
    pub path: Utf8PathBuf,
}

pub fn build_svelte<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    glob_entry: &'static str,
    glob_watch: &'static str,
) -> Handle<super::Registry<Svelte>> {
    let handle = site_config.add_task_opaque(GlobLoaderTask::new(
        glob_entry,
        glob_watch,
        move |_, file: File<Vec<u8>>| {
            let data = compile_svelte(&file.path)?;
            let rt = Runtime {};
            let path = rt.store(&data, "js")?;
            Ok((file.path, Svelte { path }))
        },
    ));

    site_config.add_task(
        (handle,),
        |_, (styles_vec,): (&Vec<(Utf8PathBuf, Svelte)>,)| super::Registry {
            map: styles_vec.iter().cloned().collect(),
        },
    )
}

fn compile_svelte(file: &Utf8Path) -> std::io::Result<Vec<u8>> {
    let svelte_path = Utf8Path::new("node_modules/svelte/compiler.cjs");
    let mut child = Command::new("deno")
        .arg("run")
        .arg("-A")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let stdin = child.stdin.as_mut().unwrap();
    stdin.write_all(b"import {compile} from '")?;
    stdin.write_all(svelte_path.as_str().as_bytes())?;
    stdin.write_all(b"'; const source = `")?;
    let content = std::fs::read_to_string(file)?;
    stdin.write_all(content.as_bytes())?;
    stdin
        .write_all(b"`; console.log(compile(source, {generate: 'dom', format: 'esm'}).js.code);")?;

    let output = child.wait_with_output()?;
    let mut esbuild = Command::new("esbuild")
        .arg("--format=esm")
        .arg("--bundle")
        .arg("--minify")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    esbuild.stdin.as_mut().unwrap().write_all(&output.stdout)?;
    let esbuild_output = esbuild.wait_with_output()?;

    Ok(esbuild_output.stdout)
}
