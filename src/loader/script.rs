use std::process::{Command, Stdio};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{Hash32, Loader, loader::generic::LoaderGenericMultifile};

pub struct Script {
    pub path: Utf8PathBuf,
}

pub fn glob_scripts(path_base: &'static str, path_glob: &'static str) -> Loader {
    Loader::with(move |_| {
        LoaderGenericMultifile::new(
            path_base,
            path_glob,
            |path| {
                let data = compile_esbuild(path);
                let hash = Hash32::hash(&data);

                Ok((hash, data))
            },
            |rt, data| {
                let path = rt.store(&data, "js")?;

                Ok(Script { path })
            },
        )
    })
}

fn compile_esbuild(file: &Utf8Path) -> Vec<u8> {
    let output = Command::new("esbuild")
        .arg(file.as_str())
        .arg("--format=esm")
        .arg("--bundle")
        .arg("--minify")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .expect("esbuild invocation failed");

    output.stdout
}
