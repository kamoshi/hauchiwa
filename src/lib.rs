#![doc = include_str!("../README.md")]
mod builder;
mod collection;
mod generator;
mod utils;
mod watch;
mod website;

use std::any::Any;
use std::collections::HashSet;
use std::fmt::Debug;
use std::sync::LazyLock;

use gray_matter::engine::{JSON, YAML};
use gray_matter::Matter;

pub use crate::collection::Collection;
pub use crate::generator::{QueryContent, Sack};
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

/// Generate the functions used to initialize content files. These functions can
/// be used to parse the front matter using engines from crate `gray_matter`.
macro_rules! matter_parser {
	($name:ident, $engine:path) => {
		#[doc = concat!(
			"This function can be used to extract metadata from a document with `D` as the frontmatter shape.\n",
			"Configured to use [`", stringify!($engine), "`] as the engine of the parser."
		)]
		pub fn $name<D>(content: &str) -> (D, String)
		where
			D: for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
		{
			// We can cache the creation of the parser
			static PARSER: LazyLock<Matter<$engine>> = LazyLock::new(Matter::<$engine>::new);

			let result = PARSER.parse_with_struct::<D>(content).unwrap();
			(
				// Just the front matter
				result.data,
				// The rest of the content
				result.content,
			)
		}
	};
}

matter_parser!(parse_matter_yaml, YAML);
matter_parser!(parse_matter_json, JSON);
