use std::{collections::HashSet, fmt::Debug, fs, path::PathBuf, sync::Arc};

use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
	builder::{InitFn, Input, InputContent, InputItem},
	Processor, ProcessorKind,
};

fn load_single<'a>(
	init: InitFn,
	exts: &'a HashSet<&'static str>,
	processors: &'a [Processor],
) -> impl Fn(Result<PathBuf, glob::GlobError>) -> Option<InputItem> + 'a {
	move |file| {
		let file = file.unwrap();
		let file = Utf8PathBuf::from_path_buf(file).expect("Filename is not valid UTF8");

		if file.is_dir() {
			return None;
		}

		let ext = file.extension()?;

		// We check if any of the assigned processors capture and transform this file.
		// If we match anything we can exit early.
		for processor in processors {
			if processor.exts.contains(ext) {
				let data = fs::read(&file).expect("Couldn't read file");
				let hash = Sha256::digest(&data).into();

				let input = match &processor.kind {
					ProcessorKind::Asset(ref fun) => {
						let content = String::from_utf8_lossy(&data);
						let asset = fun(&content);
						let slug = file.strip_prefix("content").unwrap().to_owned();

						InputItem {
							hash,
							file,
							slug,
							data: Input::Asset(asset),
						}
					}
					ProcessorKind::Image => {
						let slug = file.strip_prefix("content").unwrap().to_owned();

						InputItem {
							hash,
							file,
							slug,
							data: Input::Picture,
						}
					}
				};

				return Some(input);
			}
		}

		let item = {
			if !exts.contains(ext) {
				return None;
			}

			let data = fs::read(&file).expect("Couldn't read file");
			let hash = Sha256::digest(&data).into();
			let content = String::from_utf8_lossy(&data);
			let (meta, content) = init.call(&content);

			let area = match file.file_stem() {
				Some("index") => file
					.parent()
					.map(ToOwned::to_owned)
					.unwrap_or(file.with_extension("")),
				_ => file.with_extension(""),
			};

			let slug = area.strip_prefix("content").unwrap().to_owned();

			InputItem {
				hash,
				file,
				slug,
				data: Input::Content(InputContent {
					area,
					meta,
					text: content,
				}),
			}
		};

		Some(item)
	}
}

#[derive(Debug)]
struct LoaderGlob {
	base: &'static str,
	glob: &'static str,
	exts: HashSet<&'static str>,
}

impl LoaderGlob {
	fn load(&self, init: InitFn, processors: &[Processor]) -> Vec<InputItem> {
		let pattern = Utf8Path::new(self.base).join(self.glob);
		glob::glob(pattern.as_str())
			.expect("Invalid glob pattern")
			.filter_map(load_single(init, &self.exts, processors))
			.collect()
	}
}

#[derive(Debug)]
enum Loader {
	Glob(LoaderGlob),
}

/// An opaque representation of a source of inputs loaded into the generator.
/// You can think of a single collection as a set of written articles with
/// shared frontmatter shape, for example your blog posts.
///
/// Hovewer, a collection can also load additional files like images or custom
/// assets. This is useful when you want to colocate assets and images next to
/// the articles. A common use case is to directly reference the images relative
/// to the markdown file.
#[derive(Debug)]
pub struct Collection {
	loader: Loader,
	init: InitFn,
}

impl Collection {
	/// Create a new collection which draws content from the filesystem files
	/// via a glob pattern. Usually used to collect articles written as markdown
	/// files, however it is completely format agnostic.
	///
	/// The parameter `parse_matter` allows you to customize how the metadata
	/// should be parsed. Default functions for the most common formats are
	/// provided by library:
	/// * [`parse_matter_json`](`crate::parse_matter_json`) - parse JSON metadata
	/// * [`parse_matter_yaml`](`crate::parse_matter_yaml`) - parse YAML metadata
	///
	/// # Examples
	///
	/// ```rust
	/// Collection::glob_with("content", "posts/**/*", ["md"], parse_matter_yaml::<Post>);
	/// ```
	pub fn glob_with<T>(
		path_base: &'static str,
		path_glob: &'static str,
		exts_content: impl IntoIterator<Item = &'static str>,
		parse_matter: fn(&str) -> (T, String),
	) -> Self
	where
		T: for<'de> Deserialize<'de> + Send + Sync + 'static,
	{
		Self {
			loader: Loader::Glob(LoaderGlob {
				base: path_base,
				glob: path_glob,
				exts: HashSet::from_iter(exts_content),
			}),
			init: InitFn(Arc::new(move |content| {
				let (meta, data) = parse_matter(content);
				(Arc::new(meta), data)
			})),
		}
	}

	pub(crate) fn load(&self, processors: &[Processor]) -> Vec<InputItem> {
		match &self.loader {
			Loader::Glob(loader) => loader.load(self.init.clone(), processors),
		}
	}

	pub(crate) fn load_single(&self, path: &Utf8Path) -> Option<InputItem> {
		let Loader::Glob(loader) = &self.loader;
		let pattern = Utf8Path::new(loader.base).join(loader.glob);
		let pattern = glob::Pattern::new(pattern.as_str()).expect("Bad pattern");

		if !pattern.matches_path(path.as_std_path()) {
			return None;
		};

		glob::glob(path.as_str())
			.expect("Invalid glob pattern")
			.filter_map(load_single(self.init.clone(), &loader.exts, &[]))
			.last()
	}
}
