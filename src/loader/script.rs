use crate::{
    loader::{glob::GlobLoaderTask, Runtime},
    task::{Handle, },
    SiteConfig,
};
use camino::{Utf8Path, Utf8PathBuf};
use std::{collections::HashMap, process::{Command, Stdio}};

#[derive(Clone)]
pub struct Script {
    pub path: Utf8PathBuf,
}

#[derive(Clone)]
pub struct Scripts {
    map: HashMap<Utf8PathBuf, Script>,
}

impl Scripts {
    pub fn get(&self, path: impl AsRef<Utf8Path>) -> Option<&Script> {
        self.map.get(path.as_ref())
    }
}

pub fn build_scripts<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    entry_point_glob: &'static str,
    watch_glob: &'static str,
) -> Handle<Scripts> {
    let scripts_vec_handle: Handle<Vec<(Utf8PathBuf, Script)>> = {
        let task = GlobLoaderTask::new(entry_point_glob, watch_glob, move |_globals, file| {
            let data = compile_esbuild(&file.path)?;
            let rt = Runtime;
            let path = rt.store(&data, "js")?;
            Ok((file.path, Script { path }))
        });
        site_config.add_task_opaque(task)
    };

    site_config.add_task(
        (scripts_vec_handle,),
        |_, (scripts_vec,): (&Vec<(Utf8PathBuf, Script)>,)| Scripts {
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
