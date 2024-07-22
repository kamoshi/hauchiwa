mod content;
mod gen;
mod tree;
mod watch;
mod website;

use std::collections::{HashMap, HashSet};
use std::process::Command;

use camino::Utf8PathBuf;
use chrono::Datelike;

pub use crate::content::{Bibliography, Content, Link, LinkDate, Linkable, Outline};
pub use crate::tree::{Sack, TreePage};
pub use crate::website::Website;

#[derive(Debug, Clone, Copy)]
pub enum Mode {
	Build,
	Watch,
}

#[derive(Debug, Clone)]
pub struct BuildContext {
	pub mode: Mode,
	pub year: i32,
	pub date: String,
	pub link: String,
	pub hash: String,
}

impl BuildContext {
	fn new() -> Self {
		let time = chrono::Utc::now();
		Self {
			mode: Mode::Build,
			year: time.year(),
			date: time.format("%Y/%m/%d %H:%M").to_string(),
			link: "https://git.kamoshi.org/kamov/website".into(),
			hash: String::from_utf8(
				Command::new("git")
					.args(["rev-parse", "--short", "HEAD"])
					.output()
					.expect("Couldn't load git revision")
					.stdout,
			)
			.expect("Invalid UTF8")
			.trim()
			.into(),
		}
	}
}

impl Default for BuildContext {
	fn default() -> Self {
		Self::new()
	}
}

#[derive(Debug, Clone)]
pub struct Artifacts {
	pub images: HashMap<Utf8PathBuf, Utf8PathBuf>,
	pub styles: HashSet<Utf8PathBuf>,
	pub javascript: HashMap<String, Utf8PathBuf>,
}
