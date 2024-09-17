use std::collections::HashMap;
use std::rc::Rc;

use camino::Utf8PathBuf;

use crate::collection::Collection;
use crate::gen::build;
use crate::tree::{Output, Sack, Virtual};
use crate::watch::watch;
use crate::{Context, Mode};

/// This struct represents the website which will be built by the generator. The infividual
/// settings can be set by calling the `design` function.
#[derive(Debug)]
pub struct Website<G: Send + Sync> {
	pub(crate) loaders: Vec<Collection<G>>,
	pub(crate) dist: Utf8PathBuf,
	pub(crate) dist_js: Utf8PathBuf,
	pub(crate) special: Vec<Rc<Output<G>>>,
	pub(crate) javascript: HashMap<&'static str, &'static str>,
}

impl<G: Send + Sync + Clone + 'static> Website<G> {
	pub fn design() -> WebsiteDesigner<G> {
		WebsiteDesigner::new()
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
		watch(&ctx, &self.loaders, state, artifacts).unwrap()
	}
}

/// A builder struct for creating a `Website` with specified settings.
#[derive(Debug, Default)]
pub struct WebsiteDesigner<G: Send + Sync> {
	loaders: Vec<Collection<G>>,
	special: Vec<Rc<Output<G>>>,
	js: HashMap<&'static str, &'static str>,
}

impl<G: Send + Sync + 'static> WebsiteDesigner<G> {
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
			loaders: self.loaders,
			dist: "dist".into(),
			dist_js: "js".into(),
			special: self.special,
			javascript: self.js,
		}
	}
}
