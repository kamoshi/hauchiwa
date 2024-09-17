#![doc = include_str!("../README.md")]
mod collection;
mod content;
mod gen;
mod tree;
mod utils;
mod watch;
mod website;

pub use crate::collection::{Collection, Processor};
pub use crate::content::{Bibliography, Outline};
pub use crate::gen::store::{HashedScript, HashedStyle, Store};
pub use crate::tree::Sack;
pub use crate::website::{Website, WebsiteCreator};

#[derive(Debug, Clone, Copy)]
pub enum Mode {
	Build,
	Watch,
}

#[derive(Debug, Clone)]
pub struct Context<D: Send + Sync> {
	pub mode: Mode,
	pub data: D,
}
