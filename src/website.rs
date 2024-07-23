use std::collections::HashMap;
use std::{collections::HashSet, rc::Rc};

use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;

use crate::content::process_content;
use crate::gen::build;
use crate::tree::{Output, PipelineItem, Sack, Virtual};
use crate::watch::watch;
use crate::{BuildContext, Content, Mode};

/// This struct represents the website which will be built by the generator. The infividual
/// settings can be set by calling the `design` function.
#[derive(Debug)]
pub struct Website {
	pub(crate) dist: Utf8PathBuf,
	pub(crate) dist_js: Utf8PathBuf,
	pub(crate) sources: Vec<Source>,
	pub(crate) special: Vec<Rc<Output>>,
	pub(crate) javascript: HashMap<&'static str, &'static str>,
}

impl Website {
	pub fn design() -> WebsiteDesigner {
		WebsiteDesigner::default()
	}

	pub fn build(&self) {
		let ctx = BuildContext {
			mode: Mode::Build,
			..Default::default()
		};
		let _ = build(&ctx, self);
	}

	pub fn watch(&self) {
		let ctx = BuildContext {
			mode: Mode::Watch,
			..Default::default()
		};
		let (state, artifacts) = crate::gen::build(&ctx, self);
		watch(&ctx, &self.sources, state, &artifacts).unwrap()
	}
}

/// A builder struct for creating a `Website` with specified settings.
#[derive(Debug, Default)]
pub struct WebsiteDesigner {
	sources: Vec<Source>,
	special: Vec<Rc<Output>>,
	js: HashMap<&'static str, &'static str>,
}

impl WebsiteDesigner {
	pub fn content<T>(mut self, path: &'static str, exts: HashSet<&'static str>) -> Self
	where
		T: for<'de> Deserialize<'de> + Content + Clone + Send + Sync + 'static,
	{
		let source = Source {
			path,
			exts,
			func: process_content::<T>,
		};
		self.sources.push(source);
		self
	}

	pub fn add_virtual(mut self, func: fn(&Sack) -> String, path: Utf8PathBuf) -> Self {
		self.special.push(
			Output {
				kind: Virtual::new(func).into(),
				path,
				link: None,
			}
			.into(),
		);
		self
	}

	pub fn js(mut self, alias: &'static str, path: &'static str) -> Self {
		self.js.insert(alias, path);
		self
	}

	pub fn finish(self) -> Website {
		Website {
			dist: "dist".into(),
			dist_js: "js".into(),
			sources: self.sources,
			special: self.special,
			javascript: self.js,
		}
	}
}

#[derive(Debug)]
pub(crate) struct Source {
	pub path: &'static str,
	pub exts: HashSet<&'static str>,
	pub func: fn(PipelineItem) -> PipelineItem,
}

impl Source {
	pub(crate) fn get(&self) -> Vec<PipelineItem> {
		crate::tree::gather(self.path, &self.exts)
			.into_iter()
			.map(self.func)
			.collect()
	}

	pub(crate) fn get_maybe(&self, path: &Utf8Path) -> Option<PipelineItem> {
		let pattern = glob::Pattern::new(self.path).expect("Bad pattern");
		if !pattern.matches_path(path.as_std_path()) {
			return None;
		};

		let item = match path.is_file() {
			true => Some(crate::tree::to_source(path.to_owned(), &self.exts)),
			false => None,
		};

		item.map(Into::into).map(self.func)
	}
}
