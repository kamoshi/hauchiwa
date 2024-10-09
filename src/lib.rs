#![doc = include_str!("../README.md")]
mod builder;
mod collection;
mod content;
mod generator;
mod utils;
mod watch;
mod website;

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
