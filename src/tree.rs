//! The purpose of this module is to process the data loaded from content files, which involves
//! loading the data from hard drive, and then processing it further depending on the file type.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use hayagriva::Library;
use serde::Serialize;

use crate::content::{Link, LinkDate, Linkable};
use crate::gen::store::{HashedScript, HashedStyle, Store};
use crate::BuildContext;

/// Function objects of this type can be used to process content items.
pub(crate) type ProcessorFn = Arc<dyn Fn(PipelineItem) -> PipelineItem + Send + Sync>;

/// Marks whether the item should be treated as a content page, converted into a standalone HTML
/// page, or as a bundled asset. Only items marked as `Index` can be rendered as a page.
#[derive(Clone)]
pub(crate) enum FileItemKind {
	/// Marks items as converted to `index.html`.
	Index(ProcessorFn),
	/// Marks items as bundled.
	Bundle,
}

impl Debug for FileItemKind {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			FileItemKind::Index(ptr) => {
				todo!()
			}
			FileItemKind::Bundle => {
				todo!()
			}
		}
	}
}

/// Metadata for a single item consumed by SSG.
#[derive(Debug, Clone)]
pub(crate) struct FileItem {
	/// The kind of an item from disk.
	pub kind: FileItemKind,
	/// Original source file location.
	pub path: Utf8PathBuf,
}

#[derive(Clone)]
pub(crate) struct DeferredHtml {
	pub(crate) lazy: Arc<dyn Fn(&Sack) -> String + Send + Sync>,
}

impl Debug for DeferredHtml {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let ptr = &*self.lazy as *const dyn Fn(&Sack) -> String as *const () as usize;
		f.debug_struct("DeferredHtml").field("lazy", &ptr).finish()
	}
}

#[derive(Debug, Clone)]
/// Marks how the asset should be processed by the SSG.
pub(crate) enum AssetKind {
	/// Data renderable to HTML. In order to process the data, a closure should be called.
	Html(DeferredHtml),
	/// Bibliographical data.
	Bibtex(Library),
	/// Image. For now they are simply cloned to the `dist` director.
	Image,
}

impl AssetKind {
	pub fn html(f: impl Fn(&Sack) -> String + Send + Sync + 'static) -> Self {
		Self::Html(DeferredHtml { lazy: Arc::new(f) })
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
pub(crate) enum PipelineItem {
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
	/// Processed artifacts (styles, scripts, etc.)
	pub store: &'a Store,
	/// Literally all of the content
	pub hole: &'a [&'a Output],
	/// Current path for the page being rendered
	pub path: &'a Utf8PathBuf,
	/// Original file location for this page
	pub file: Option<&'a Utf8PathBuf>,
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

	pub fn get_import_map(&self) -> String {
		let ok = self
			.store
			.javascript
			.iter()
			.map(|(k, v)| (k.clone(), v.path.clone()))
			.collect();
		let map = ImportMap { imports: &ok };

		serde_json::to_string(&map).unwrap()
	}

	pub fn get_script(&self, alias: &str) -> Option<&HashedScript> {
		self.store.javascript.get(alias)
	}

	/// Get compiled CSS style by alias.
	pub fn get_style(&self, alias: &str) -> Option<&HashedStyle> {
		self.store.styles.get(alias)
	}

	/// Get optimized image path by original path.
	pub fn get_image(&self, alias: &Utf8Path) -> Option<&Utf8Path> {
		self.store.images.get(alias).map(AsRef::as_ref)
	}
}

#[derive(Debug, Serialize)]
pub struct ImportMap<'a> {
	imports: &'a HashMap<String, Utf8PathBuf>,
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
