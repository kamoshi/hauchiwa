use std::process::{Command, Stdio};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{Hash32, Loader, loader::generic::LoaderGenericMultifile};

/// Represents a compiled JavaScript module ready for inclusion in the output site.
///
/// Each [`Script`] corresponds to a `.js`, `.ts`, or `.tsx` source file that has been
/// bundled and minified using `esbuild`. The compiled output is stored under a
/// hashed filename, and the `path` can be used in templates.
pub struct Script {
    // Path to the compiled `.js` file,
    pub path: Utf8PathBuf,
}

/// Constructs a loader that compiles JavaScript/TypeScript files using `esbuild`.
///
/// Matches files based on a glob pattern and compiles each into a minified
/// ES module using `esbuild` with bundling enabled. Each output is content-hashed
/// and written to disk. The resulting `Script` provides the path to that output.
///
/// ### Parameters
/// - `path_base`: Base directory for relative resolution of the glob.
/// - `path_glob`: Glob pattern used to select `.js`, `.ts`, etc. source files.
///
/// ### Returns
/// A `Loader` that emits `Script` objects keyed by hashed output content.
///
/// ### Requirements
/// - `esbuild` must be installed and available on `$PATH`.
///
/// ### Example
/// ```rust
/// use hauchiwa::loader::glob_scripts;
///
/// let loader = glob_scripts("src/scripts", "**/*.ts");
/// ```
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
