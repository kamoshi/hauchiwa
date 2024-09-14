use std::{collections::HashSet, fmt::Debug, fs, sync::Arc};

use camino::{Utf8Path, Utf8PathBuf};
use glob::glob;
use gray_matter::{engine::YAML, Matter};
use hayagriva::Library;
use serde::Deserialize;

use crate::{
	tree::{Asset, FileItem, FileItemKind, Output, PipelineItem, ProcessorFn},
	Bibliography, Linkable, Outline, Sack,
};

#[derive(Clone, Copy)]
pub struct Processor<D> {
	/// Convert a single document to HTML.
	pub read_content:
		fn(&str, &Sack, &Utf8Path, Option<&Library>) -> (String, Outline, Bibliography),
	/// Render the website page for this document.
	pub to_html: fn(&D, &str, &Sack, Outline, Bibliography) -> String,
	/// Get link for this content
	pub to_link: fn(&D, path: Utf8PathBuf) -> Option<Linkable>,
}

impl<D> Processor<D>
where
	D: for<'de> Deserialize<'de> + Send + Sync + 'static,
{
	fn init(self) -> impl Fn(PipelineItem) -> PipelineItem {
		let Processor {
			read_content,
			to_html,
			to_link,
		} = self;

		move |item| {
			let meta = match item {
				PipelineItem::Skip(e) if matches!(e.kind, FileItemKind::Index(..)) => e,
				_ => return item,
			};

			let dir = meta.path.parent().unwrap().strip_prefix("content").unwrap();
			let dir = match meta.path.file_stem().unwrap() {
				"index" => dir.to_owned(),
				name => dir.join(name),
			};
			let path = dir.join("index.html");

			let data = fs::read_to_string(&meta.path).unwrap();
			let (metadata, content) = parse_meta::<D>(&data);
			let link = to_link(&metadata, Utf8Path::new("/").join(&dir));

			Output {
				kind: Asset {
					kind: crate::tree::AssetKind::html(move |sack| {
						let library = sack.get_library();
						let (parsed, outline, bibliography) =
							read_content(&content, sack, &dir, library);
						to_html(&metadata, &parsed, sack, outline, bibliography)
					}),
					meta,
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

pub enum Loader {
	Glob {
		base: &'static str,
		glob: &'static str,
		exts: HashSet<&'static str>,
		func: ProcessorFn,
	},
}

impl Loader {
	pub fn glob_with<D>(
		base: &'static str,
		glob: &'static str,
		exts: HashSet<&'static str>,
		processor: Processor<D>,
	) -> Self
	where
		D: for<'de> Deserialize<'de> + Send + Sync + 'static,
	{
		Self::Glob {
			base,
			glob,
			exts,
			func: Arc::new(processor.init()),
		}
	}

	pub(crate) fn get_maybe(&self, path: &Utf8Path) -> Option<PipelineItem> {
		let (pattern, exts, func) = match self {
			Loader::Glob {
				base,
				glob,
				exts,
				func,
			} => (Utf8Path::new(base).join(glob), exts, func),
		};

		let pattern = glob::Pattern::new(pattern.as_str()).expect("Bad pattern");
		if !pattern.matches_path(path.as_std_path()) {
			return None;
		};

		let item = match path.is_file() {
			true => Some(to_source(path.to_owned(), exts, func.clone())),
			false => None,
		};

		item.map(Into::into)
	}

	pub(crate) fn load(&self) -> Vec<FileItem> {
		match self {
			Loader::Glob {
				base,
				glob,
				exts,
				func,
			} => {
				let glob = Utf8Path::new(base).join(glob);
				let srcs = load_glob(glob.as_str(), exts, func);
				srcs
			}
		}
	}
}

impl Debug for Loader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		todo!()
	}
}

fn to_source(path: Utf8PathBuf, exts: &HashSet<&'static str>, func: ProcessorFn) -> FileItem {
	let has_ext = path.extension().map_or(false, |ext| exts.contains(ext));

	FileItem {
		kind: if has_ext {
			FileItemKind::Index(func)
		} else {
			FileItemKind::Bundle
		},
		path,
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
