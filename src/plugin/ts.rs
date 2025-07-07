use std::process::{Command, Stdio};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    Hash32,
    plugin::{Loadable, generic::LoaderGenericMultifile},
};

pub struct Script {
    pub path: Utf8PathBuf,
}

pub(crate) fn new_loader_ts(path_base: &'static str, path_glob: &'static str) -> impl Loadable {
    LoaderGenericMultifile::new(
        path_base,
        path_glob,
        |path| {
            let data = compile_esbuild(path);
            let hash = Hash32::hash(&data);

            (hash, data)
        },
        |rt, data| {
            let path = rt.store(&data, "js").unwrap();

            Script { path }
        },
    )
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
