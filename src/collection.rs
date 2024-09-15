use std::{collections::HashSet, fmt::Debug, fs, sync::Arc};

use camino::{Utf8Path, Utf8PathBuf};
use glob::glob;
use gray_matter::{engine::YAML, Matter};
use hayagriva::Library;
use serde::Deserialize;

use crate::{
	tree::{Asset, FileItem, FileItemBundle, FileItemIndex, Output, PipelineItem, ProcessorFn},
	Bibliography, Linkable, Outline, Sack,
};

type ReaderFn = fn(&str, &Sack, &Utf8Path, Option<&Library>) -> (String, Outline, Bibliography);

#[derive(Clone, Copy)]
pub struct Processor<D> {
	/// Convert a single document to HTML.
	pub read_content: ReaderFn,
	/// Render the website page for this document.
	pub to_html: fn(&D, &str, &Sack, Outline, Bibliography) -> String,
	/// Get link for this content
	pub to_link: fn(&D, path: Utf8PathBuf) -> Option<Linkable>,
}

impl<D> Processor<D>
where
	D: for<'de> Deserialize<'de> + Send + Sync + 'static,
{
	fn init(self) -> impl Fn(FileItemIndex) -> PipelineItem {
		let Processor {
			read_content,
			to_html,
			to_link,
		} = self;

		move |index| {
			let dir = index
				.path
				.parent()
				.unwrap()
				.strip_prefix("content")
				.unwrap();
			let dir = match index.path.file_stem().unwrap() {
				"index" => dir.to_owned(),
				name => dir.join(name),
			};
			let path = dir.join("index.html");

			let data = fs::read_to_string(&index.path).unwrap();
			let (meta, content) = parse_meta::<D>(&data);
			let meta = Arc::new(meta);

			let link = to_link(&meta, Utf8Path::new("/").join(&dir));

			Output {
				kind: Asset {
					kind: crate::tree::AssetKind::html(meta.clone(), move |sack| {
						let library = sack.get_library();
						let (parsed, outline, bibliography) =
							read_content(&content, sack, &dir, library);
						to_html(&meta, &parsed, sack, outline, bibliography)
					}),
					meta: FileItem::Index(index),
				}
				.into(),
				path,
				link,
			}
			.into()
		}
	}
}

/// Extract front matter from a document with `D` as the metadata shape.
fn parse_meta<D>(raw: &str) -> (D, String)
where
	D: for<'de> Deserialize<'de>,
{
	let parser = Matter::<YAML>::new();
	let result = parser.parse_with_struct::<D>(raw).unwrap();

	(
		// Just the front matter
		result.data,
		// The rest of the content
		result.content,
	)
}

struct LoaderGlob {
	base: &'static str,
	glob: &'static str,
	exts: HashSet<&'static str>,
	func: ProcessorFn,
}

enum Loader {
	Glob(LoaderGlob),
}

pub struct Collection(Loader);

impl Collection {
	/// Collect file items from file system for further processing.
	pub fn glob_with<D>(
		base: &'static str,
		glob: &'static str,
		exts: HashSet<&'static str>,
		processor: Processor<D>,
	) -> Self
	where
		D: for<'de> Deserialize<'de> + Send + Sync + 'static,
	{
		Self(Loader::Glob(LoaderGlob {
			base,
			glob,
			exts,
			func: Arc::new(processor.init()),
		}))
	}

	pub(crate) fn get_maybe(&self, path: &Utf8Path) -> Option<PipelineItem> {
		let Loader::Glob(loader) = &self.0;

		let pattern = Utf8Path::new(loader.base).join(loader.glob);
		let closure = loader.func.clone();

		let pattern = glob::Pattern::new(pattern.as_str()).expect("Bad pattern");
		if !pattern.matches_path(path.as_std_path()) {
			return None;
		};

		let item = match path.is_file() {
			true => to_source(path.to_owned(), &loader.exts, closure.clone()),
			false => return None,
		};

		match item {
			FileItem::Index(index) => Some(closure(index)),
			FileItem::Bundle(_) => None,
		}
	}

	pub(crate) fn load(&self) -> Vec<FileItem> {
		match &self.0 {
			Loader::Glob(loader) => {
				let glob = Utf8Path::new(loader.base).join(loader.glob);
				let srcs = load_glob(glob.as_str(), &loader.exts, &loader.func);
				srcs
			}
		}
	}
}

impl Debug for Collection {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Collection").finish()
	}
}

fn to_source(path: Utf8PathBuf, exts: &HashSet<&'static str>, func: ProcessorFn) -> FileItem {
	let has_ext = path.extension().map_or(false, |ext| exts.contains(ext));

	match has_ext {
		true => FileItem::Index(FileItemIndex { path, func }),
		false => FileItem::Bundle(FileItemBundle { path }),
	}
}

fn load_glob(pattern: &str, exts: &HashSet<&'static str>, func: &ProcessorFn) -> Vec<FileItem> {
	glob(pattern)
		.expect("Invalid glob pattern")
		.filter_map(|path| {
			let path = path.unwrap();
			let path = Utf8PathBuf::from_path_buf(path).expect("Filename is not valid UTF8");

			match path.is_dir() {
				true => None,
				false => to_source(path, exts, func.clone()).into(),
			}
		})
		.collect()
}
