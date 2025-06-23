use std::process::{Command, Stdio};

use camino::Utf8Path;

pub mod assets;
pub mod content;

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
