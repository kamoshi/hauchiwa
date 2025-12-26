use camino::{Utf8Path, Utf8PathBuf};
use std::process::{Command, Stdio};

use crate::{SiteConfig, error::HauchiwaError, loader::glob::GlobRegistryTask, task::Handle};

#[derive(Clone)]
pub struct JS {
    pub path: Utf8PathBuf,
}

impl<G> SiteConfig<G>
where
    G: Send + Sync + 'static,
{
    pub fn build_scripts(
        &mut self,
        glob_entry: &'static str,
        glob_watch: &'static str,
    ) -> Result<Handle<super::Registry<JS>>, HauchiwaError> {
        Ok(self.add_task_opaque(GlobRegistryTask::new(
            vec![glob_entry],
            vec![glob_watch],
            move |_, rt, file| {
                let data = compile_esbuild(&file.path)?;
                let path = rt.store(&data, "js")?;

                Ok((file.path, JS { path }))
            },
        )?))
    }
}

fn compile_esbuild(file: &Utf8Path) -> anyhow::Result<Vec<u8>> {
    let output = match Command::new("esbuild")
        .arg(file.as_str())
        .arg("--format=esm")
        .arg("--bundle")
        .arg("--minify")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            anyhow::bail!("Failed to compile JavaScript file with Esbuild ({})", err);
        }
    };

    Ok(output.stdout)
}
