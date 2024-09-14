use camino::Utf8PathBuf;
use chrono::{DateTime, Utc};

pub struct Outline(pub Vec<(String, String)>);

pub struct Bibliography(pub Option<Vec<String>>);

#[derive(Debug, Clone)]
pub struct Link {
	pub path: Utf8PathBuf,
	pub name: String,
	pub desc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinkDate {
	pub link: Link,
	pub date: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum Linkable {
	Link(Link),
	Date(LinkDate),
}
