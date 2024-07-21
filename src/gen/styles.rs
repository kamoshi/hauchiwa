use std::time::Instant;
use std::{collections::HashSet, path::PathBuf};
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use glob::GlobError;
use grass::Options;
use rayon::iter::{ParallelBridge, ParallelIterator};

pub(crate) fn build_css() -> HashSet<Utf8PathBuf> {
	println!("Compiling styles...");
	let now = Instant::now();
	let styles = compile_all();
	println!("Compiled styles in {:.2?}", now.elapsed());
	styles
}

fn compile_all() -> HashSet<Utf8PathBuf> {
	glob::glob("styles/**/[!_]*.scss")
		.expect("Failed to read glob pattern")
		.par_bridge()
		.filter_map(compile)
		.collect()
}

fn compile(entry: Result<PathBuf, GlobError>) -> Option<Utf8PathBuf> {
	match entry {
		Ok(path) => {
			let opts = Options::default();
			let css = grass::from_path(&path, &opts).unwrap();

			let name = path.file_stem().unwrap().to_string_lossy();
			let path = format!("dist/{}.css", name);

			fs::write(path, css).unwrap();

			Some(Utf8Path::new("/").join(name.as_ref()).with_extension("css"))
		}
		Err(e) => {
			eprintln!("{:?}", e);
			None
		}
	}
}
