mod tree;
mod site;
mod watch;
mod gen;

use std::collections::{HashMap, HashSet};
use std::{fs, process::Command};

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Datelike, Utc};
use gray_matter::Matter;
use gray_matter::engine::YAML;
use hypertext::{Raw, Renderable};
use serde::Deserialize;
use tree::{Asset, FileItemKind, Output, PipelineItem};

pub use crate::tree::{Content, Outline, Sack, TreePage};
pub use crate::site::Website;

#[derive(Debug, Clone, Copy)]
pub enum Mode {
	Build,
	Watch,
}

#[derive(Debug, Clone)]
pub struct BuildContext {
	pub mode: Mode,
	pub year: i32,
	pub date: String,
	pub link: String,
	pub hash: String,
}

impl BuildContext {
	fn new() -> Self {
		let time = chrono::Utc::now();
		Self {
			mode: Mode::Build,
			year: time.year(),
			date: time.format("%Y/%m/%d %H:%M").to_string(),
			link: "https://git.kamoshi.org/kamov/website".into(),
			hash: String::from_utf8(
				Command::new("git")
					.args(["rev-parse", "--short", "HEAD"])
					.output()
					.expect("Couldn't load git revision")
					.stdout,
			)
			.expect("Invalid UTF8")
			.trim()
			.into(),
		}
	}
}

impl Default for BuildContext {
	fn default() -> Self {
		Self::new()
	}
}

#[derive(Debug, Clone)]
pub struct Link {
	pub path: Utf8PathBuf,
	pub name: String,
	pub desc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinkDate {
	pub link: Link,
	pub date: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum Linkable {
	Link(Link),
	Date(LinkDate),
}

pub fn process_content<T>(item: PipelineItem) -> PipelineItem
where
	T: for<'de> Deserialize<'de> + Content + Clone + Send + Sync + 'static,
{
	let meta = match item {
		PipelineItem::Skip(e) if matches!(e.kind, FileItemKind::Index) => e,
		_ => return item,
	};

	let dir = meta.path.parent().unwrap().strip_prefix("content").unwrap();
	let dir = match meta.path.file_stem().unwrap() {
		"index" => dir.to_owned(),
		name => dir.join(name),
	};
	let path = dir.join("index.html");

	match meta.path.extension() {
		Some("md" | "mdx" | "lhs") => {
			let raw = fs::read_to_string(&meta.path).unwrap();
			let (matter, parsed) = parse_frontmatter::<T>(&raw);
			let link = T::as_link(&matter, Utf8Path::new("/").join(&dir));

			Output {
				kind: Asset {
					kind: crate::tree::AssetKind::html(move |sack| {
						let lib = sack.get_library();
						let (outline, parsed, bib) = T::parse(
							parsed.clone(),
							lib,
							dir.clone(),
							sack.artifacts.images.clone()
						);
						T::render(matter.clone(), sack, Raw(parsed), outline, bib)
							.render()
							.into()
					}),
					meta,
				}
				.into(),
				path,
				link,
			}
			.into()
		}
		_ => meta.into(),
	}
}

fn parse_frontmatter<D>(raw: &str) -> (D, String)
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

#[derive(Debug, Clone)]
pub struct Artifacts {
	pub images: HashMap<Utf8PathBuf, Utf8PathBuf>,
	pub styles: HashSet<Utf8PathBuf>,
	pub javascript: HashMap<String, Utf8PathBuf>,
}
