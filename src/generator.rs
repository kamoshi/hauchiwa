use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use camino::{Utf8Path, Utf8PathBuf};
use glob::GlobError;
use hayagriva::Library;
use sha2::{Digest, Sha256};

use crate::builder::{Builder, Input, InputItem, InputStylesheet, Scheduler};
use crate::{Collection, Context, Website};

#[derive(Debug)]
pub struct QueryContent<'a, D> {
	pub file: &'a Utf8Path,
	pub slug: &'a Utf8Path,
	pub area: &'a Utf8Path,
	pub meta: &'a D,
	pub content: &'a str,
}

/// This struct allows for querying the website hierarchy. It is passed to each rendered website
/// page, so that it can easily access the website metadata.
pub struct Sack<'a, G: Send + Sync> {
	/// Global `Context` for the current build.
	pub(crate) context: &'a Context<G>,
	/// Builder allows scheduling build requests.
	pub(crate) builder: Arc<RwLock<Builder>>,
	/// Tracked dependencies for current instantation.
	pub(crate) tracked: Rc<RefCell<HashMap<Utf8PathBuf, Vec<u8>>>>,
	/// Every single input.
	pub(crate) items: &'a HashMap<Utf8PathBuf, InputItem>,
}

impl<'a, G: Send + Sync> Sack<'a, G> {
	/// Retrieve global context
	pub fn get_context(&self) -> &Context<G> {
		self.context
	}

	pub fn get_content<D: 'static>(&self, pattern: &str) -> Option<QueryContent<'_, D>> {
		let pattern = glob::Pattern::new(pattern).expect("Bad glob pattern");
		let input = self
			.items
			.values()
			.find(|item| pattern.matches_path(item.slug.as_ref()))?;
		if !matches!(input.data, Input::Content(..)) {
			return None;
		}

		self.tracked
			.borrow_mut()
			.insert(input.file.clone(), input.hash.clone());

		let (area, meta, content) = match &input.data {
			Input::Content(input_content) => {
				let area = input_content.area.as_ref();
				let meta = input_content.meta.downcast_ref::<D>()?;
				let data = input_content.content.as_str();
				Some((area, meta, data))
			}
			_ => unreachable!(),
		}?;

		Some(QueryContent {
			file: &input.file,
			slug: &input.slug,
			area,
			meta,
			content,
		})
	}

	/// Retrieve many possible content items.
	pub fn get_content_list<D: 'static>(&self, pattern: &str) -> Vec<QueryContent<'_, D>> {
		let pattern = glob::Pattern::new(pattern).expect("Bad glob pattern");
		let inputs: Vec<_> = self
			.items
			.values()
			.filter(|item| pattern.matches_path(item.slug.as_ref()))
			.collect();

		let mut tracked = self.tracked.borrow_mut();
		for input in inputs.iter() {
			tracked.insert(input.file.clone(), input.hash.clone());
		}

		inputs
			.into_iter()
			.filter_map(|item| {
				let (area, meta, content) = match &item.data {
					Input::Content(input_content) => {
						let area = input_content.area.as_ref();
						let meta = input_content.meta.downcast_ref::<D>()?;
						let data = input_content.content.as_str();
						Some((area, meta, data))
					}
					_ => None,
				}?;

				Some(QueryContent {
					file: &item.file,
					slug: &item.slug,
					area,
					meta,
					content,
				})
			})
			.collect()
	}

	/// Get compiled CSS style by alias.
	pub fn get_styles(&self, path: &Utf8Path) -> Option<Utf8PathBuf> {
		let input = self.items.values().find(|item| item.file == path)?;
		if !matches!(input.data, Input::Stylesheet(..)) {
			return None;
		}

		self.tracked
			.borrow_mut()
			.insert(input.file.clone(), input.hash.clone());

		self.schedule(input)
	}

	/// Get optimized image path by original path.
	pub fn get_picture(&self, path: &Utf8Path) -> Option<Utf8PathBuf> {
		let input = self.items.values().find(|item| item.file == path)?;
		if !matches!(input.data, Input::Picture) {
			return Some(path.to_owned());
		}

		self.tracked
			.borrow_mut()
			.insert(input.file.clone(), input.hash.clone());

		self.schedule(input)
	}

	pub fn get_script(&self, path: &str) -> Option<Utf8PathBuf> {
		let path = Utf8Path::new(".cache/scripts/")
			.join(path)
			.with_extension("js");

		let input = self.items.values().find(|item| item.file == path)?;
		if !matches!(input.data, Input::Script) {
			return None;
		}

		self.tracked
			.borrow_mut()
			.insert(input.file.clone(), input.hash.clone());

		self.schedule(input)
	}

	pub fn get_library(&self, area: &Utf8Path) -> Option<&Library> {
		let glob = format!("{}/*.bib", area);
		let glob = glob::Pattern::new(&glob).expect("Bad glob pattern");
		let opts = glob::MatchOptions {
			case_sensitive: true,
			require_literal_separator: true,
			require_literal_leading_dot: false,
		};

		let input = self
			.items
			.values()
			.find(|item| glob.matches_path_with(item.file.as_std_path(), opts))?;

		if !matches!(input.data, Input::Library(..)) {
			return None;
		}

		self.tracked
			.borrow_mut()
			.insert(input.file.clone(), input.hash.clone());

		match input.data {
			Input::Library(ref library) => Some(&library.library),
			_ => unreachable!(),
		}
	}

	fn schedule(&self, input: &InputItem) -> Option<Utf8PathBuf> {
		let res = self.builder.read().unwrap().check(input);
		if res.is_some() {
			return res;
		}

		let res = self.builder.write().unwrap().build(input);
		Some(res)
	}
}

pub(crate) fn build<'a, G>(website: &'a Website<G>, context: &'a Context<G>) -> Scheduler<'a, G>
where
	G: Send + Sync + 'static,
{
	clean_dist();
	build_static();

	let items: Vec<_> = website
		.collections
		.iter()
		.flat_map(Collection::load)
		.chain(load_styles(&website.global_styles))
		.chain(load_scripts(&website.global_scripts))
		.collect();

	let mut scheduler = Scheduler::new(website, context, items);
	scheduler.build();

	build_pagefind("dist".into());

	scheduler
}

pub(crate) fn clean_dist() {
	println!("Cleaning dist");
	if fs::metadata("dist").is_ok() {
		fs::remove_dir_all("dist").unwrap();
	}
	fs::create_dir("dist").unwrap();
}

pub(crate) fn load_styles(paths: &[Utf8PathBuf]) -> Vec<InputItem> {
	paths
		.iter()
		.filter_map(|path| glob::glob(path.join("**/[!_]*.scss").as_str()).ok())
		.flatten()
		.filter_map(compile)
		.collect()
}

fn compile(entry: Result<PathBuf, GlobError>) -> Option<InputItem> {
	match entry {
		Ok(file) => {
			let file = Utf8PathBuf::try_from(file).expect("Invalid UTF-8 file name");
			let opts = grass::Options::default().style(grass::OutputStyle::Compressed);
			let stylesheet = grass::from_path(&file, &opts).unwrap();
			let hash = Vec::from_iter(Sha256::digest(&stylesheet));

			Some(InputItem {
				hash,
				file: file.clone(),
				slug: file,
				data: Input::Stylesheet(InputStylesheet { stylesheet }),
			})
		}
		Err(e) => {
			eprintln!("{:?}", e);
			None
		}
	}
}

pub(crate) fn build_static() {
	copy_rec(Path::new("public"), Path::new("dist")).unwrap();
}

pub(crate) fn copy_rec(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
	fs::create_dir_all(&dst)?;
	for entry in fs::read_dir(src)? {
		let entry = entry?;
		let filetype = entry.file_type()?;
		if filetype.is_dir() {
			copy_rec(entry.path(), dst.as_ref().join(entry.file_name()))?;
		} else {
			fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
		}
	}
	Ok(())
}

fn load_scripts(entrypoints: &HashMap<&str, &str>) -> Vec<InputItem> {
	let mut cmd = Command::new("esbuild");

	for (alias, path) in entrypoints.iter() {
		cmd.arg(format!("{}={}", alias, path));
	}

	let path_scripts = Utf8Path::new(".cache/scripts/");

	let res = cmd
		.arg("--format=esm")
		.arg("--bundle")
		.arg("--minify")
		.arg(format!("--outdir={}", path_scripts))
		.output()
		.unwrap();

	let stderr = String::from_utf8(res.stderr).unwrap();
	println!("{}", stderr);

	entrypoints
		.keys()
		.map(|key| {
			let file = path_scripts.join(key).with_extension("js");
			let buffer = fs::read(&file).unwrap();
			let hash = Vec::from_iter(Sha256::digest(buffer));

			InputItem {
				slug: file.clone(),
				file,
				hash,
				data: Input::Script,
			}
		})
		.collect()
}

pub(crate) fn build_pagefind(out: &Utf8Path) {
	let res = Command::new("pagefind")
		.args(["--site", out.as_str()])
		.output()
		.unwrap();

	println!("{}", String::from_utf8(res.stdout).unwrap());
}
