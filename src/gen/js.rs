//! This module provides functionality to build JavaScript bundles using `esbuild`. The `build_js`
//! function takes an entry point map, invokes `esbuild` with the specified configuration, and
//! outputs the bundled JavaScript files to the `dist/js/` directory.

use std::collections::HashMap;
use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};

pub(crate) fn build_js(
	js: &HashMap<&str, &str>,
	out: &Utf8Path,
	dir: &Utf8Path,
) -> HashMap<String, Utf8PathBuf> {
	let mut cmd = Command::new("esbuild");

	for (alias, path) in js.iter() {
		cmd.arg(format!("{}={}", alias, path));
	}

	let res = cmd
		.arg("--format=esm")
		.arg("--bundle")
		.arg("--splitting")
		.arg("--minify")
		.arg(format!("--outdir={}", out.join(dir)))
		.output()
		.unwrap();

	let stderr = String::from_utf8(res.stderr).unwrap();
	println!("{}", stderr);

	js.keys()
		.map(|key| {
			(
				key.to_string(),
				Utf8Path::new("/").join(dir).join(key).with_extension("js"),
			)
		})
		.collect()
}
