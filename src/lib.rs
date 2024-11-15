#![doc = include_str!("../README.md")]
mod builder;
mod collection;
mod content;
mod generator;
mod utils;
mod watch;
mod website;

use std::any::Any;
use std::collections::HashSet;
use std::fmt::Debug;

use gray_matter::engine::YAML;
use gray_matter::Matter;
use serde::Deserialize;

pub use crate::collection::Collection;
pub use crate::content::Bibliography;
pub use crate::generator::Sack;
pub use crate::website::{Website, WebsiteCreator};

/// This value controls whether the library should run in the *build* or the
/// *watch* mode. In *build* mode, the library builds every page of the website
/// just once and stops. In *watch* mode, the library initializes the initial
/// state of the build process, opens up a websocket port, and watches for any
/// changes in the file system. Using the *watch* mode allows you to enable
/// live-reload while editing the styles or the content of your website.
#[derive(Debug, Clone, Copy)]
pub enum Mode {
	/// Run the library in *build* mode.
	Build,
	/// Run the library in *watch* mode.
	Watch,
}

#[derive(Debug, Clone)]
pub struct Context<D: Send + Sync> {
	pub mode: Mode,
	pub data: D,
}

type Erased = Box<dyn Any + Send + Sync>;

pub(crate) enum ProcessorKind {
	Asset(Box<dyn Fn(&str) -> Erased>),
	Image,
}

impl Debug for ProcessorKind {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ProcessorKind::Asset(_) => write!(f, "<Processor Asset>"),
			ProcessorKind::Image => write!(f, "<Processor Image>"),
		}
	}
}

#[derive(Debug)]
pub struct Processor {
	exts: HashSet<&'static str>,
	kind: ProcessorKind,
}

impl Processor {
	pub fn process_assets<T: Send + Sync + 'static>(
		exts: impl IntoIterator<Item = &'static str>,
		call: fn(&str) -> T,
	) -> Self {
		Self {
			exts: HashSet::from_iter(exts),
			kind: ProcessorKind::Asset(Box::new(move |data| Box::new(call(data)))),
		}
	}

	pub fn process_images(exts: impl IntoIterator<Item = &'static str>) -> Self {
		Self {
			exts: HashSet::from_iter(exts),
			kind: ProcessorKind::Image,
		}
	}
}

/// This function can be used to extract front-matter from a document with `D`
/// as the metadata shape.
pub fn parse_matter<T>(content: &str) -> (T, String)
where
	T: for<'de> Deserialize<'de> + Send + Sync + 'static,
{
	// TODO: it might be more optimal to save the parser in closure
	let parser = Matter::<YAML>::new();
	let result = parser.parse_with_struct::<T>(content).unwrap();
	(
		// Just the front matter
		result.data,
		// The rest of the content
		result.content,
	)
}
