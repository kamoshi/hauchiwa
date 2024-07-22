use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use gray_matter::{engine::YAML, Matter};
use hayagriva::Library;
use serde::Deserialize;

use crate::tree::{Asset, FileItemKind, Output, PipelineItem, Sack};

/// Represents a piece of content that can be rendered as a page. This trait needs to be
/// implemented for the front matter associated with some web page as that is what ultimately
/// matters when rendering the page. Each front matter definition maps to exactly one kind of
/// rendered page on the website.
pub trait Content {
	/// Extract front matter from a document.
	fn parse_metadata(data: &str) -> (Self, String)
	where
		Self: Sized + for<'de> Deserialize<'de>,
	{
		parse_metadata_default::<Self>(data)
	}

	/// Convert a single markdown document to HTML.
	fn parse_content(
		content: &str,
		sack: &Sack,
		path: &Utf8Path,
		library: Option<&Library>,
	) -> (String, Outline, Bibliography);

	/// Render the website page for this document.
	fn as_html(
		&self,
		parsed: &str,
		sack: &Sack,
		outline: Outline,
		bibliography: Bibliography,
	) -> String;

	/// Get link for this content
	fn as_link(&self, path: Utf8PathBuf) -> Option<Linkable>;
}

pub struct Outline(pub Vec<(String, String)>);

pub struct Bibliography(pub Option<Vec<String>>);

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

fn parse_metadata_default<D>(raw: &str) -> (D, String)
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

pub(crate) fn process_content<T>(item: PipelineItem) -> PipelineItem
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

	let data = fs::read_to_string(&meta.path).unwrap();
	let (metadata, content) = T::parse_metadata(&data);
	let link = T::as_link(&metadata, Utf8Path::new("/").join(&dir));

	Output {
		kind: Asset {
			kind: crate::tree::AssetKind::html(move |sack| {
				let library = sack.get_library();
				let (parsed, outline, bibliography) =
					T::parse_content(&content, sack, &dir, library);
				T::as_html(&metadata, &parsed, sack, outline, bibliography)
			}),
			meta,
		}
		.into(),
		path,
		link,
	}
	.into()
}
