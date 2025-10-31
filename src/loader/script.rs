use crate::{
    SiteConfig,
    loader::{Runtime, glob::GlobLoaderTask},
    task::Handle,
};
use camino::{Utf8Path, Utf8PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone)]
pub struct JS {
    pub path: Utf8PathBuf,
}

pub fn build_scripts<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    glob_entry: &'static str,
    glob_watch: &'static str,
) -> Handle<super::Registry<JS>> {
    let scripts_vec_handle = site_config.add_task_opaque(GlobLoaderTask::new(
        glob_entry,
        glob_watch,
        move |_, file| {
            let data = compile_esbuild(&file.path)?;
            let rt = Runtime;
            let path = rt.store(&data, "js")?;
            Ok((file.path, JS { path }))
        },
    ));

    site_config.add_task(
        (scripts_vec_handle,),
        |_, (scripts_vec,): (&Vec<(Utf8PathBuf, JS)>,)| super::Registry {
            map: scripts_vec.iter().cloned().collect(),
        },
    )
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
