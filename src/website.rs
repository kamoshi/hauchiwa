use std::collections::HashMap;
use std::rc::Rc;

use camino::Utf8PathBuf;

use crate::collection::Collection;
use crate::gen::build;
use crate::tree::{Output, Sack, Virtual};
use crate::watch::watch;
use crate::{BuildContext, Mode};

/// This struct represents the website which will be built by the generator. The infividual
/// settings can be set by calling the `design` function.
#[derive(Debug)]
pub struct Website {
	pub(crate) loaders: Vec<Collection>,
	pub(crate) dist: Utf8PathBuf,
	pub(crate) dist_js: Utf8PathBuf,
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
		watch(&ctx, &self.loaders, state, artifacts).unwrap()
	}
}

/// A builder struct for creating a `Website` with specified settings.
#[derive(Debug, Default)]
pub struct WebsiteDesigner {
	loaders: Vec<Collection>,
	special: Vec<Rc<Output>>,
	js: HashMap<&'static str, &'static str>,
}

impl WebsiteDesigner {
	pub fn add_loaders(mut self, loaders: impl IntoIterator<Item = Collection>) -> Self {
		self.loaders.extend(loaders);
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
			loaders: self.loaders,
			dist: "dist".into(),
			dist_js: "js".into(),
			special: self.special,
			javascript: self.js,
		}
	}
}
