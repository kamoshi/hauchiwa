#![doc = include_str!("../README.md")]
mod builder;
mod collection;
mod content;
mod generator;
mod utils;
mod watch;
mod website;

pub use crate::collection::Collection;
pub use crate::content::{Bibliography, Outline};
pub use crate::generator::Sack;
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
