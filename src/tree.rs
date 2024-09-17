//! The purpose of this module is to process the data loaded from content files, which involves
//! loading the data from hard drive, and then processing it further depending on the file type.

use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use hayagriva::Library;
use serde::Serialize;

use crate::gen::store::{HashedScript, HashedStyle, Store};
use crate::Context;

/// Function objects of this type can be used to process content items.
/// This type erases the front matter type, so that it's completely opaque.
pub(crate) type ProcessorFn<G> = Arc<dyn Fn(FileItemIndex<G>) -> PipelineItem<G> + Send + Sync>;

/// Filesystem item renderable to a HTML page.
#[derive(Clone)]
pub(crate) struct FileItemIndex<D: Send + Sync> {
	/// Original source file location.
	pub(crate) path: Utf8PathBuf,
	/// Processor function closure.
	pub(crate) func: ProcessorFn<D>,
}

impl<D: Send + Sync> FileItemIndex<D> {
	pub(crate) fn process(self) -> PipelineItem<D> {
		(self.func.clone())(self)
	}
}

impl<D: Send + Sync> Debug for FileItemIndex<D> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FileItemIndex")
			.field("path", &self.path)
			.field("func", &"[closure]".to_string())
			.finish()
	}
}

/// Item bundled to some page
#[derive(Debug, Clone)]
pub(crate) struct FileItemBundle {
	/// Original source file location.
	pub path: Utf8PathBuf,
}

/// Metadata for a single item consumed by SSG.
#[derive(Debug, Clone)]
pub(crate) enum FileItem<D: Send + Sync> {
	/// Items marked as converted to `index.html`.
	Index(FileItemIndex<D>),
	/// Items marked as bundled.
	Bundle(FileItemBundle),
}

impl<D: Send + Sync> FileItem<D> {
	#[inline(always)]
	pub(crate) fn get_path(&self) -> &Utf8Path {
		match self {
			FileItem::Index(index) => index.path.as_ref(),
			FileItem::Bundle(bundle) => bundle.path.as_ref(),
		}
	}
}

#[derive(Clone)]
pub(crate) struct DeferredHtml<D: Send + Sync> {
	/// Any front matter
	pub(crate) meta: Arc<dyn Any + Send + Sync>,
	pub(crate) lazy: Arc<dyn Fn(&Sack<D>) -> String + Send + Sync>,
}

impl<D: Send + Sync> Debug for DeferredHtml<D> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let ptr = &*self.lazy as *const dyn Fn(&Sack<_>) -> String as *const () as usize;
		f.debug_struct("DeferredHtml").field("lazy", &ptr).finish()
	}
}

#[derive(Debug, Clone)]
/// Marks how the asset should be processed by the SSG.
pub(crate) enum AssetKind<D: Send + Sync> {
	/// Data renderable to HTML. In order to process the data, a closure should be called.
	Html(DeferredHtml<D>),
	/// Bibliographical data.
	Bibtex(Library),
	/// Image. For now they are simply cloned to the `dist` director.
	Image,
}

impl<D: Send + Sync> AssetKind<D> {
	pub fn html<M, F>(meta: Arc<M>, f: F) -> Self
	where
		M: Any + Send + Sync,
		F: Fn(&Sack<D>) -> String + Send + Sync + 'static,
	{
		Self::Html(DeferredHtml {
			meta,
			lazy: Arc::new(f),
		})
	}
}

/// Asset corresponding to a file on disk.
#[derive(Debug)]
pub(crate) struct Asset<D: Send + Sync> {
	/// The kind of a processed asset.
	pub kind: AssetKind<D>,
	/// File metadata
	pub meta: FileItem<D>,
}

/// Dynamically generated asset not corresponding to any file on disk. This is useful when the
/// generated page is not a content page, e.g. page list.
pub(crate) struct Virtual<D: Send + Sync>(pub Box<dyn Fn(&Sack<D>) -> String + Send + Sync>);

impl<D: Send + Sync> Virtual<D> {
	pub fn new(call: impl Fn(&Sack<D>) -> String + Send + Sync + 'static) -> Self {
		Self(Box::new(call))
	}
}

impl<D: Send + Sync> Debug for Virtual<D> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		// rust mental gymnastics moment
		let ptr = &*self.0 as *const dyn Fn(&Sack<D>) -> String as *const () as usize;
		f.debug_tuple("Virtual").field(&ptr).finish()
	}
}

/// The kind of an output item.
#[derive(Debug)]
pub(crate) enum OutputKind<D: Send + Sync> {
	/// Marks an output item which corresponds to a file on disk.
	Asset(Asset<D>),
	/// Marks an output item which doesn't correspond to any file.
	Virtual(Virtual<D>),
}

impl<D: Send + Sync> From<Asset<D>> for OutputKind<D> {
	fn from(value: Asset<D>) -> Self {
		OutputKind::Asset(value)
	}
}

impl<D: Send + Sync> From<Virtual<D>> for OutputKind<D> {
	fn from(value: Virtual<D>) -> Self {
		OutputKind::Virtual(value)
	}
}

/// Renderable output
#[derive(Debug)]
pub(crate) struct Output<D: Send + Sync> {
	/// The kind of an output item
	pub(crate) kind: OutputKind<D>,
	/// Path for the output in dist
	pub(crate) path: Utf8PathBuf,
}

/// Items currently in the pipeline. In order for an item to be rendered, it needs to be marked as
/// `Take`, which means it needs to have an output location assigned to itself.
#[derive(Debug)]
pub(crate) enum PipelineItem<D: Send + Sync> {
	/// Unclaimed file.
	Skip(FileItem<D>),
	/// Data ready to be processed.
	Take(Output<D>),
}

impl<D: Send + Sync> From<FileItem<D>> for PipelineItem<D> {
	fn from(value: FileItem<D>) -> Self {
		Self::Skip(value)
	}
}

impl<D: Send + Sync> From<Output<D>> for PipelineItem<D> {
	fn from(value: Output<D>) -> Self {
		Self::Take(value)
	}
}

impl<D: Send + Sync> From<PipelineItem<D>> for Option<Output<D>> {
	fn from(value: PipelineItem<D>) -> Self {
		match value {
			PipelineItem::Skip(_) => None,
			PipelineItem::Take(e) => Some(e),
		}
	}
}

/// This struct allows for querying the website hierarchy. It is passed to each rendered website
/// page, so that it can easily access the website metadata.
pub struct Sack<'a, D: Send + Sync> {
	/// TODO: make Sack parametric over this type
	pub ctx: &'a Context<D>,
	/// Current path for the page being rendered
	pub path: &'a Utf8Path,
	/// Processed artifacts (styles, scripts, etc.)
	pub(crate) store: &'a Store,
	/// Original file location for this page
	pub(crate) file: Option<&'a Utf8Path>,
	/// All of the content on the page.
	pub(crate) hole: &'a [&'a Output<D>],
}

impl<'a, D: Send + Sync> Sack<'a, D> {
	pub fn get_meta<M: 'static>(&self, pattern: &str) -> Vec<(&Utf8Path, &M)> {
		let pattern = glob::Pattern::new(pattern).expect("Bad glob pattern");

		self.hole
			.iter()
			.filter(|item| pattern.matches_path(item.path.as_ref()))
			.filter_map(|item| {
				let path = item.path.as_ref();
				let meta = match &item.kind {
					OutputKind::Asset(Asset {
						kind: AssetKind::Html(DeferredHtml { meta, .. }),
						..
					}) => meta.downcast_ref::<M>(),
					_ => None,
				};

				meta.map(|meta| (path, meta))
			})
			.collect()
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
		self.file
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
