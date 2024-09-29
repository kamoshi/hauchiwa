use std::collections::HashMap;

use camino::Utf8PathBuf;

use crate::builder::Task;
use crate::collection::Collection;
use crate::generator::{build, Sack};
// use crate::watch::watch;
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
	/// All collections added to this website.
	pub(crate) collections: Vec<Collection>,
	/// Build tasks which can be used to generate pages.
	pub(crate) tasks: Vec<Task<G>>,
	// other
	pub(crate) dist_js: Utf8PathBuf,
	pub(crate) javascript: HashMap<&'static str, &'static str>,
}

impl<G: Send + Sync + 'static> Website<G> {
	pub fn setup() -> WebsiteCreator<G> {
		WebsiteCreator::new()
	}

	pub fn build(&self, global: G) {
		let context = Context {
			mode: Mode::Build,
			data: global,
		};
		let _ = build(self, &context);
	}

	// pub fn watch(&self, global: G) {
	// 	let ctx = Context {
	// 		mode: Mode::Watch,
	// 		data: global,
	// 	};
	// 	let (state, artifacts) = crate::gen::build(&ctx, self);
	// 	watch(&ctx, &self.collections, state, artifacts).unwrap()
	// }
}

/// A builder struct for creating a `Website` with specified settings.
#[derive(Debug, Default)]
pub struct WebsiteCreator<G: Send + Sync> {
	collections: Vec<Collection>,
	tasks: Vec<Task<G>>,
	js: HashMap<&'static str, &'static str>,
}

impl<G: Send + Sync + 'static> WebsiteCreator<G> {
	fn new() -> Self {
		Self {
			collections: Vec::default(),
			tasks: Vec::default(),
			js: HashMap::default(),
		}
	}

	pub fn add_collections(mut self, collections: impl IntoIterator<Item = Collection>) -> Self {
		self.collections.extend(collections);
		self
	}

	pub fn add_scripts(
		mut self,
		scripts: impl IntoIterator<Item = (&'static str, &'static str)>,
	) -> Self {
		self.js.extend(scripts);
		self
	}

	pub fn add_task(mut self, func: fn(Sack<G>) -> Vec<(Utf8PathBuf, String)>) -> Self {
		self.tasks.push(Task::new(func));
		self
	}

	pub fn finish(self) -> Website<G> {
		Website {
			dir_dist: "dist".into(),
			collections: self.collections,
			tasks: self.tasks,
			dist_js: "js".into(),
			javascript: self.js,
		}
	}
}
