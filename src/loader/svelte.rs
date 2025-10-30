use crate::{
    loader::{glob::GlobLoaderTask, File, Runtime},
    task::Handle,
    SiteConfig,
};
use camino::{Utf8Path, Utf8PathBuf};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[derive(Clone)]
pub struct Svelte {
    pub path: Utf8PathBuf,
}

pub fn glob_svelte<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path_base: &'static str,
    path_glob: &'static str,
) -> Handle<Vec<Svelte>> {
    let task = GlobLoaderTask::new(path_base, path_glob, move |_globals, file: File<Vec<u8>>| {
        let data = compile_svelte(&file.path)?;
        let rt = Runtime {};
        let path = rt.store(&data, "js")?;
        Ok(Svelte { path })
    });
    site_config.add_task_opaque(task)
}

pub fn build_svelte<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    entry_point: &'static str,
    watch_glob: &'static str,
) -> Handle<Svelte> {
    let task = GlobLoaderTask::new(entry_point, watch_glob, move |_globals, file: File<Vec<u8>>| {
        let data = compile_svelte(&file.path)?;
        let rt = Runtime {};
        let path = rt.store(&data, "js")?;
        Ok(Svelte { path })
    });
    site_config.add_task_opaque(task)
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
    stdin.write_all(b"`; console.log(compile(source, {generate: 'dom', format: 'esm'}).js.code);")?;

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
