use std::fmt::Debug;
use std::fs;
use std::io::Write;
use std::sync::Arc;
use std::{any::Any, collections::HashMap};

use camino::{Utf8Path, Utf8PathBuf};
use gray_matter::{engine::YAML, Matter};
use serde::Deserialize;

use crate::generator::Sack;

/// Init pointer used to dynamically retrieve front matter. The type of front matter
/// needs to be erased at run time and this is one way of accomplishing this,
/// it's hidden behind the `dyn Fn` existential type.
type InitFnPtr = Arc<dyn Fn(&str) -> (Arc<dyn Any>, String)>;

/// Wraps `InitFnPtr` and implements `Debug` trait for function pointer.
#[derive(Clone)]
pub(crate) struct InitFn(InitFnPtr);

impl InitFn {
	/// Create new `InitFn` for a given front-matter shape. This function can be used to
	/// extract front-matter from a document with `D` as the metadata shape.
	pub(crate) fn new<D>() -> Self
	where
		D: for<'de> Deserialize<'de> + 'static,
	{
		InitFn(Arc::new(|content| {
			// TODO: it might be more optimal to save the parser in closure
			let parser = Matter::<YAML>::new();
			let result = parser.parse_with_struct::<D>(content).unwrap();
			(
				// Just the front matter
				Arc::new(result.data),
				// The rest of the content
				result.content,
			)
		}))
	}

	/// Call the contained `InitFn` pointer.
	pub(crate) fn call(&self, data: &str) -> (Arc<dyn Any>, String) {
		(self.0)(data)
	}
}

impl Debug for InitFn {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Processor(*)")
	}
}

#[derive(Debug)]
pub(crate) struct InputContent {
	pub(crate) init: InitFn,
	pub(crate) meta: Arc<dyn Any>,
	pub(crate) content: String,
}

#[derive(Debug)]
pub(crate) struct InputLibrary {
	pub(crate) library: hayagriva::Library,
}

#[derive(Debug)]
pub(crate) struct InputStylesheet {
	pub(crate) stylesheet: String,
}

#[derive(Debug)]
pub(crate) enum Input {
	Content(InputContent),
	Library(InputLibrary),
	Picture,
	Stylesheet(InputStylesheet),
}

#[derive(Debug)]
pub(crate) struct InputItem {
	pub(crate) hash: Vec<u8>,
	pub(crate) file: Utf8PathBuf,
	pub(crate) slug: Utf8PathBuf,
	pub(crate) data: Input,
}

/// Task function pointer used to dynamically generate a website page.
type TaskFnPtr<G> = Arc<dyn Fn(Sack<G>) -> Vec<(Utf8PathBuf, String)>>;

/// Wraps `TaskFnPtr` and implements `Debug` trait for function pointer.
#[derive(Clone)]
pub(crate) struct Task<G: Send + Sync>(TaskFnPtr<G>);

impl<G: Send + Sync> Task<G> {
	/// Create new task function pointer.
	pub(crate) fn new<F>(func: F) -> Self
	where
		F: Fn(Sack<G>) -> Vec<(Utf8PathBuf, String)> + 'static,
	{
		Self(Arc::new(func))
	}

	/// **IO** Run the task to generate a page.
	pub(crate) fn run(&self, sack: Sack<G>) {
		let func = &*self.0;

		for (path, data) in func(sack) {
			let path = Utf8Path::new("dist").join(path);
			if let Some(dir) = path.parent() {
				fs::create_dir_all(dir).unwrap();
			}
			let mut file = fs::File::create(&path).unwrap();
			file.write_all(data.as_bytes()).unwrap();
			println!("HTML: {}", path);
		}
	}
}

impl<G: Send + Sync> Debug for Task<G> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Task(*)")
	}
}

pub(crate) struct Scheduler {
	state: HashMap<Vec<u8>, Utf8PathBuf>,
}

impl Scheduler {
	pub(crate) fn new() -> Self {
		Self {
			state: HashMap::new(),
		}
	}

	/** **Pure** */
	pub(crate) fn check(&self, input: &InputItem) -> Option<Utf8PathBuf> {
		match &input.data {
			Input::Content(input_content) => None,
			Input::Library(input_library) => None,
			Input::Picture => None,
			Input::Stylesheet(input_stylesheet) => self.state.get(&input.hash).cloned(),
		}
	}

	/** **IO** */
	pub(crate) fn build(&mut self, input: &InputItem) -> Utf8PathBuf {
		match &input.data {
			Input::Content(input_content) => "".into(),
			Input::Library(input_library) => "".into(),
			Input::Picture => "".into(),
			Input::Stylesheet(stylesheet) => {
				let hash = crate::utils::hex(&input.hash);
				let path = Utf8Path::new("hash").join(&hash).with_extension("css");

				let path_root = Utf8Path::new("/").join(&path);
				let path_dist = Utf8Path::new("dist").join(&path);

				println!("CSS: {}", path_dist);
				fs::create_dir_all(path_dist.parent().unwrap_or(&path_dist)).unwrap();
				fs::write(&path_dist, &stylesheet.stylesheet).unwrap();

				self.state.insert(input.hash.clone(), path_root.clone());
				path_root
			}
		}
	}
}
