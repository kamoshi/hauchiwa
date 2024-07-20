//! The purpose of this module is to process the data loaded from content files, which involves
//! loading the data from hard drive, and then processing it further depending on the file type.

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::usize;

use camino::{Utf8Path, Utf8PathBuf};
use glob::glob;
use hayagriva::Library;
use hypertext::Renderable;

use crate::{BuildContext, Link, LinkDate, Linkable};

pub struct Outline(pub Vec<(String, String)>);

/// Represents a piece of content that can be rendered as a page. This trait needs to be
/// implemented for the front matter associated with some web page as that is what ultimately
/// matters when rendering the page. Each front matter *definition* maps to exactly one kind of
/// rendered page on the website.
pub trait Content {
	/// Parse the document. Pass an optional library for bibliography.
	/// This generates the initial HTML markup from content.
	fn parse(
		document: String,
		library: Option<&Library>,
		path: Utf8PathBuf,
		hash: HashMap<Utf8PathBuf, Utf8PathBuf>,
	) -> (Outline, String, Option<Vec<String>>);

	/// Render the full page from parsed content.
	fn render<'s, 'p, 'html>(
		self,
		sack: &'s Sack,
		parsed: impl Renderable + 'p,
		outline: Outline,
		bib: Option<Vec<String>>,
	) -> impl Renderable + 'html
	where
		's: 'html,
		'p: 'html;

	/// Get link for this content
	fn as_link(&self, path: Utf8PathBuf) -> Option<Linkable>;
}

/// Marks whether the item should be treated as a content page, converted into a standalone HTML
/// page, or as a bundled asset.
#[derive(Debug, Clone)]
pub(crate) enum FileItemKind {
	/// Marks items converted to `index.html`.
	Index,
	/// Marks items from bundle.
	Bundle,
}

/// Metadata for a single item consumed by SSG.
#[derive(Debug, Clone)]
pub(crate) struct FileItem {
	/// The kind of an item from disk.
	pub kind: FileItemKind,
	/// Original source file location.
	pub path: Utf8PathBuf,
}

/// Marks how the asset should be processed by the SSG.
pub(crate) enum AssetKind {
	/// Data renderable to HTML. In order to process the data, a closure should be called.
	Html(Box<dyn Fn(&Sack) -> String + Send + Sync>),
	/// Bibliographical data.
	Bibtex(Library),
	/// Image. For now they are simply cloned to the `dist` director.
	Image,
}

impl Debug for AssetKind {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Html(fun) => {
				// rust mental gymnastics moment
				let ptr = &**fun as *const dyn Fn(&Sack) -> String as *const () as usize;
				f.debug_tuple("Html").field(&ptr).finish()
			}
			Self::Bibtex(b) => f.debug_tuple("Bibtex").field(b).finish(),
			Self::Image => write!(f, "Image"),
		}
	}
}

impl AssetKind {
	pub fn html(f: impl Fn(&Sack) -> String + Send + Sync + 'static) -> Self {
		Self::Html(Box::new(f))
	}
}

/// Asset corresponding to a file on disk.
#[derive(Debug)]
pub(crate) struct Asset {
	/// The kind of a processed asset.
	pub kind: AssetKind,
	/// File metadata
	pub meta: FileItem,
}

/// Dynamically generated asset not corresponding to any file on disk. This is useful when the
/// generated page is not a content page, e.g. page list.
pub(crate) struct Virtual(pub Box<dyn Fn(&Sack) -> String + Send + Sync>);

impl Virtual {
	pub fn new(call: impl Fn(&Sack) -> String + Send + Sync + 'static) -> Self {
		Self(Box::new(call))
	}
}

impl Debug for Virtual {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		// rust mental gymnastics moment
		let ptr = &*self.0 as *const dyn Fn(&Sack) -> String as *const () as usize;
		f.debug_tuple("Virtual").field(&ptr).finish()
	}
}

/// The kind of an output item.
#[derive(Debug)]
pub(crate) enum OutputKind {
	/// Marks an output item which corresponds to a file on disk.
	Asset(Asset),
	/// Marks an output item which doesn't correspond to any file.
	Virtual(Virtual),
}

impl From<Asset> for OutputKind {
	fn from(value: Asset) -> Self {
		OutputKind::Asset(value)
	}
}

impl From<Virtual> for OutputKind {
	fn from(value: Virtual) -> Self {
		OutputKind::Virtual(value)
	}
}

/// Renderable output
#[derive(Debug)]
pub(crate) struct Output {
	/// The kind of an output item
	pub(crate) kind: OutputKind,
	/// Path for the output in dist
	pub(crate) path: Utf8PathBuf,
	/// Optional URL data for outputted page.
	pub(crate) link: Option<Linkable>,
}

/// Items currently in the pipeline. In order for an item to be rendered, it needs to be marked as
/// `Take`, which means it needs to have an output location assigned to itself.
#[derive(Debug)]
pub enum PipelineItem {
	/// Unclaimed file.
	Skip(FileItem),
	/// Data ready to be processed.
	Take(Output),
}

impl From<FileItem> for PipelineItem {
	fn from(value: FileItem) -> Self {
		Self::Skip(value)
	}
}

impl From<Output> for PipelineItem {
	fn from(value: Output) -> Self {
		Self::Take(value)
	}
}

impl From<PipelineItem> for Option<Output> {
	fn from(value: PipelineItem) -> Self {
		match value {
			PipelineItem::Skip(_) => None,
			PipelineItem::Take(e) => Some(e),
		}
	}
}

/// This struct allows for querying the website hierarchy. It is passed to each rendered website
/// page, so that it can easily access the website metadata.
pub struct Sack<'a> {
	pub ctx: &'a BuildContext,
	/// Literally all of the content
	pub hole: &'a [&'a Output],
	/// Current path for the page being rendered
	pub path: &'a Utf8PathBuf,
	/// Original file location for this page
	pub file: Option<&'a Utf8PathBuf>,
	/// Hashed optimized images
	pub hash: Option<HashMap<Utf8PathBuf, Utf8PathBuf>>,
}

impl<'a> Sack<'a> {
	pub fn get_links(&self, path: &str) -> Vec<LinkDate> {
		let pattern = glob::Pattern::new(path).expect("Bad glob pattern");
		self.hole
			.iter()
			.filter(|item| pattern.matches_path(item.path.as_ref()))
			.filter_map(|item| match &item.link {
				Some(Linkable::Date(link)) => Some(link.clone()),
				_ => None,
			})
			.collect()
	}

	pub fn get_tree(&self, path: &str) -> TreePage {
		let glob = glob::Pattern::new(path).expect("Bad glob pattern");
		let list = self
			.hole
			.iter()
			.filter(|item| glob.matches_path(item.path.as_ref()))
			.filter_map(|item| match &item.link {
				Some(Linkable::Link(link)) => Some(link.clone()),
				_ => None,
			});

		let mut tree = TreePage::new();
		for link in list {
			tree.add_link(&link);
		}

		tree
	}

	pub fn get_library(&self) -> Option<&Library> {
		let glob = format!("{}/*.bib", self.path.parent()?);
		let glob = glob::Pattern::new(&glob).expect("Bad glob pattern");
		let opts = glob::MatchOptions {
			case_sensitive: true,
			require_literal_separator: true,
			require_literal_leading_dot: false,
		};

		self.hole
			.iter()
			.filter(|item| glob.matches_path_with(item.path.as_ref(), opts))
			.filter_map(|asset| match asset.kind {
				OutputKind::Asset(ref real) => Some(real),
				_ => None,
			})
			.find_map(|asset| match asset.kind {
				AssetKind::Bibtex(ref lib) => Some(lib),
				_ => None,
			})
	}

	/// Get the path for original file location
	pub fn get_file(&self) -> Option<&'a Utf8Path> {
		self.file.map(Utf8PathBuf::as_ref)
	}
}

#[derive(Debug)]
pub struct TreePage {
	pub link: Option<Link>,
	pub subs: HashMap<String, TreePage>,
}

impl TreePage {
	fn new() -> Self {
		TreePage {
			link: None,
			subs: HashMap::new(),
		}
	}

	fn add_link(&mut self, link: &Link) {
		let mut ptr = self;
		for part in link.path.iter().skip(1) {
			ptr = ptr.subs.entry(part.to_string()).or_insert(TreePage::new());
		}
		ptr.link = Some(link.clone());
	}
}

pub fn gather(pattern: &str, exts: &HashSet<&'static str>) -> Vec<PipelineItem> {
	glob(pattern)
		.expect("Invalid glob pattern")
		.filter_map(|path| {
			let path = path.unwrap();
			let path = Utf8PathBuf::from_path_buf(path).expect("Filename is not valid UTF8");

			match path.is_dir() {
				true => None,
				false => Some(to_source(path, exts)),
			}
		})
		.map(Into::into)
		.collect()
}

pub(crate) fn to_source(path: Utf8PathBuf, exts: &HashSet<&'static str>) -> FileItem {
	let hit = path.extension().map_or(false, |ext| exts.contains(ext));

	let kind = match hit {
		true => FileItemKind::Index,
		false => FileItemKind::Bundle,
	};

	FileItem { kind, path }
}
