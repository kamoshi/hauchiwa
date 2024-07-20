use std::{collections::HashSet, rc::Rc};

use camino::{Utf8Path, Utf8PathBuf};

use crate::tree::{Output, PipelineItem, Sack, Virtual};
use crate::{BuildContext, Mode};

#[derive(Debug)]
pub struct Website {
	sources: Vec<Source>,
	special: Vec<Rc<Output>>,
}

impl Website {
	pub fn new() -> WebsiteBuilder {
		WebsiteBuilder::default()
	}

	pub fn build(&self) {
		let ctx = BuildContext {
			mode: Mode::Build,
			..Default::default()
		};
		let _ = crate::build::build(&ctx, &self.sources, &self.special.clone());
	}

	pub fn watch(&self) {
		let ctx = BuildContext {
			mode: Mode::Watch,
			..Default::default()
		};
		let state = crate::build::build(&ctx, &self.sources, &self.special.clone());
		crate::watch::watch(&ctx, &self.sources, state).unwrap()
	}
}

#[derive(Debug, Default)]
pub struct WebsiteBuilder {
	sources: Vec<Source>,
	special: Vec<Rc<Output>>,
}

impl WebsiteBuilder {
	pub fn add_source(
		mut self,
		path: &'static str,
		exts: HashSet<&'static str>,
		func: fn(PipelineItem) -> PipelineItem,
	) -> Self {
		self.sources.push(Source { path, exts, func });
		self
	}

	pub fn add_virtual(
		mut self,
		func: fn(&Sack) -> String,
		path: Utf8PathBuf,
	) -> Self {
		self.special.push(Output {
			kind: Virtual::new(func).into(),
			path,
			link: None,
		}.into());
		self
	}

	pub fn finish(self) -> Website {
		Website {
			sources: self.sources,
			special: self.special,
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
