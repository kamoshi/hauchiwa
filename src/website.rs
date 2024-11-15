use std::collections::HashMap;

use camino::Utf8PathBuf;

use crate::builder::Task;
use crate::collection::Collection;
use crate::generator::{build, Sack};
use crate::watch::watch;
use crate::{Context, Mode, Processor};

/// This struct represents the website which will be built by the generator. The individual
/// settings can be set by calling the `setup` function.
///
/// The `G` type parameter is the global data container accessible in every page renderer as `ctx.data`,
/// though it can be replaced with the `()` Unit if you don't need to pass any data.
#[derive(Debug)]
pub struct Website<G: Send + Sync> {
	/// Rendered assets and content are outputted to this directory.
	/// All collections added to this website.
	pub(crate) collections: Vec<Collection>,
	/// Preprocessors for files
	pub(crate) processors: Vec<Processor>,
	/// Build tasks which can be used to generate pages.
	pub(crate) tasks: Vec<Task<G>>,
	/// Global scripts
	pub(crate) global_scripts: HashMap<&'static str, &'static str>,
	/// Global styles
	pub(crate) global_styles: Vec<Utf8PathBuf>,
	/// Sitemap options
	pub(crate) opts_sitemap: Option<Utf8PathBuf>,
}

impl<G: Send + Sync + 'static> Website<G> {
	pub fn setup() -> WebsiteCreator<G> {
		WebsiteCreator::new()
	}

	pub fn build(&self, data: G) {
		let _ = build(
			self,
			&Context {
				mode: Mode::Build,
				data,
			},
		);
	}

	pub fn watch(&self, data: G) {
		let context = Context {
			mode: Mode::Watch,
			data,
		};

		let scheduler = build(self, &context);
		watch(self, scheduler).unwrap()
	}
}

/// A builder struct for creating a `Website` with specified settings.
#[derive(Debug, Default)]
pub struct WebsiteCreator<G: Send + Sync> {
	collections: Vec<Collection>,
	processors: Vec<Processor>,
	tasks: Vec<Task<G>>,
	global_scripts: HashMap<&'static str, &'static str>,
	global_styles: Vec<Utf8PathBuf>,
	opts_sitemap: Option<Utf8PathBuf>,
}

impl<G: Send + Sync + 'static> WebsiteCreator<G> {
	fn new() -> Self {
		Self {
			collections: Vec::default(),
			processors: Vec::default(),
			tasks: Vec::default(),
			global_scripts: HashMap::default(),
			global_styles: Vec::default(),
			opts_sitemap: None,
		}
	}

	pub fn add_collections(mut self, collections: impl IntoIterator<Item = Collection>) -> Self {
		self.collections.extend(collections);
		self
	}

	pub fn add_processors(mut self, processors: impl IntoIterator<Item = Processor>) -> Self {
		self.processors.extend(processors);
		self
	}

	pub fn add_scripts(
		mut self,
		scripts: impl IntoIterator<Item = (&'static str, &'static str)>,
	) -> Self {
		self.global_scripts.extend(scripts);
		self
	}

	pub fn add_styles(mut self, paths: impl IntoIterator<Item = Utf8PathBuf>) -> Self {
		self.global_styles.extend(paths);
		self
	}

	pub fn add_task(mut self, func: fn(Sack<G>) -> Vec<(Utf8PathBuf, String)>) -> Self {
		self.tasks.push(Task::new(func));
		self
	}

	pub fn set_opts_sitemap(mut self, path: impl AsRef<str>) -> Self {
		self.opts_sitemap = Some(path.as_ref().into());
		self
	}

	pub fn finish(self) -> Website<G> {
		Website {
			collections: self.collections,
			processors: self.processors,
			tasks: self.tasks,
			global_scripts: self.global_scripts,
			global_styles: self.global_styles,
			opts_sitemap: self.opts_sitemap,
		}
	}
}
