use std::process::{Command, Stdio};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    loader::{BundleLoaderTask, File, FileLoaderTask, Runtime},
    task::Handle,
    SiteConfig,
};

/// Represents a compiled JavaScript module ready for inclusion in the output site.
///
/// Each [`Script`] corresponds to a `.js`, `.ts`, or `.tsx` source file that has been
/// bundled and minified using `esbuild`. The compiled output is stored under a
/// hashed filename, and the `path` can be used in templates.
#[derive(Clone)]
pub struct Script {
    // Path to the compiled `.js` file,
    pub path: Utf8PathBuf,
}

pub fn glob_scripts<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path_base: &'static str,
    path_glob: &'static str,
) -> Handle<Vec<Script>> {
    let task = FileLoaderTask::new(path_base, path_glob, move |_globals, file| {
        let data = compile_esbuild(&file.path)?;
        let rt = Runtime;
        let path = rt.store(&data, "js")?;
        Ok(Script { path })
    });
    site_config.add_task_opaque(task)
}

pub fn build_script<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    entry_point: &'static str,
    watch_glob: &'static str,
) -> Handle<Script> {
    let task = BundleLoaderTask::new(entry_point, watch_glob, move |_globals, file| {
        let data = compile_esbuild(&file.path)?;
        let rt = Runtime;
        let path = rt.store(&data, "js")?;
        Ok(Script { path })
    });
    site_config.add_task_opaque(task)
}

fn compile_esbuild(file: &Utf8Path) -> std::io::Result<Vec<u8>> {
    let output = Command::new("esbuild")
        .arg(file.as_str())
        .arg("--format=esm")
        .arg("--bundle")
        .arg("--minify")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()?;

    Ok(output.stdout)
}
