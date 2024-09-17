use std::collections::HashMap;
use std::rc::Rc;

use camino::Utf8PathBuf;

use crate::collection::Collection;
use crate::gen::build;
use crate::tree::{Output, Sack, Virtual};
use crate::watch::watch;
use crate::{Context, Mode};

/// This struct represents the website which will be built by the generator. The individual
/// settings can be set by calling the `setup` function.
///
/// The `G` type parameter is the global data container accessible in every page renderer as `ctx.data`,
/// though it can be replaced with the `()` Unit if you don't need to pass any data.
#[derive(Debug)]
pub struct Website<G: Send + Sync> {
	/// Rendered assets and content are outputted to this directory.
	pub(crate) dir_dist: Utf8PathBuf,
	/// All collections added to this website. The collections are the source of rendered pages.
	pub(crate) collections: Vec<Collection<G>>,
	pub(crate) dist_js: Utf8PathBuf,
	pub(crate) special: Vec<Rc<Output<G>>>,
	pub(crate) javascript: HashMap<&'static str, &'static str>,
}

impl<G: Send + Sync + 'static> Website<G> {
	pub fn setup() -> WebsiteCreator<G> {
		WebsiteCreator::new()
	}

	pub fn build(&self, global: G) {
		let ctx = Context {
			mode: Mode::Build,
			data: global,
		};
		let _ = build(&ctx, self);
	}

	pub fn watch(&self, global: G) {
		let ctx = Context {
			mode: Mode::Watch,
			data: global,
		};
		let (state, artifacts) = crate::gen::build(&ctx, self);
		watch(&ctx, &self.collections, state, artifacts).unwrap()
	}
}

/// A builder struct for creating a `Website` with specified settings.
#[derive(Debug, Default)]
pub struct WebsiteCreator<G: Send + Sync> {
	loaders: Vec<Collection<G>>,
	special: Vec<Rc<Output<G>>>,
	js: HashMap<&'static str, &'static str>,
}

impl<G: Send + Sync + 'static> WebsiteCreator<G> {
	fn new() -> Self {
		Self {
			loaders: Vec::default(),
			special: Vec::default(),
			js: HashMap::default(),
		}
	}

	pub fn add_collections(mut self, collections: impl IntoIterator<Item = Collection<G>>) -> Self {
		self.loaders.extend(collections);
		self
	}

	pub fn add_scripts(
		mut self,
		scripts: impl IntoIterator<Item = (&'static str, &'static str)>,
	) -> Self {
		self.js.extend(scripts);
		self
	}

	pub fn add_virtual(mut self, func: fn(&Sack<G>) -> String, path: Utf8PathBuf) -> Self {
		self.special.push(
			Output {
				kind: Virtual::new(func).into(),
				path,
			}
			.into(),
		);
		self
	}

	pub fn finish(self) -> Website<G> {
		Website {
			dir_dist: "dist".into(),
			collections: self.loaders,
			dist_js: "js".into(),
			special: self.special,
			javascript: self.js,
		}
	}
}
