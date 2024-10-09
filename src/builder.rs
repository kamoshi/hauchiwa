use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::io::Write;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::{fs, mem};

use camino::{Utf8Path, Utf8PathBuf};
use gray_matter::{engine::YAML, Matter};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::Deserialize;
use sitemap_rs::url::{ChangeFrequency, Url};
use sitemap_rs::url_set::UrlSet;

use crate::generator::Sack;
use crate::{Context, Website};

/// Init pointer used to dynamically retrieve front matter. The type of front matter
/// needs to be erased at run time and this is one way of accomplishing this,
/// it's hidden behind the `dyn Fn` existential type.
type InitFnPtr = Arc<dyn Fn(&str) -> (Arc<dyn Any + Send + Sync>, String)>;

/// Wraps `InitFnPtr` and implements `Debug` trait for function pointer.
#[derive(Clone)]
pub(crate) struct InitFn(InitFnPtr);

impl InitFn {
	/// Create new `InitFn` for a given front-matter shape. This function can be used to
	/// extract front-matter from a document with `D` as the metadata shape.
	pub(crate) fn new<D>() -> Self
	where
		D: for<'de> Deserialize<'de> + Send + Sync + 'static,
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
	pub(crate) fn call(&self, data: &str) -> (Arc<dyn Any + Send + Sync>, String) {
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
	pub(crate) area: Utf8PathBuf,
	pub(crate) meta: Arc<dyn Any + Send + Sync>,
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
	Script,
}

#[derive(Debug)]
pub(crate) struct InputItem {
	pub(crate) hash: Vec<u8>,
	pub(crate) file: Utf8PathBuf,
	pub(crate) slug: Utf8PathBuf,
	pub(crate) data: Input,
}

/// Task function pointer used to dynamically generate a website page.
type TaskFnPtr<G> = Arc<dyn Fn(Sack<G>) -> Vec<(Utf8PathBuf, String)> + Send + Sync>;

/// Wraps `TaskFnPtr` and implements `Debug` trait for function pointer.
pub(crate) struct Task<G: Send + Sync>(TaskFnPtr<G>);

impl<G: Send + Sync> Task<G> {
	/// Create new task function pointer.
	pub(crate) fn new<F>(func: F) -> Self
	where
		F: Fn(Sack<G>) -> Vec<(Utf8PathBuf, String)> + Send + Sync + 'static,
	{
		Self(Arc::new(func))
	}

	/// Run the task to generate a page.
	pub(crate) fn run(&self, sack: Sack<G>) -> Vec<(Utf8PathBuf, String)> {
		(self.0)(sack)
	}
}

impl<G: Send + Sync> Clone for Task<G> {
	fn clone(&self) -> Self {
		Self(self.0.clone())
	}
}

impl<G: Send + Sync> Debug for Task<G> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Task(*)")
	}
}

#[derive(Debug)]
struct Trace<G: Send + Sync> {
	task: Task<G>,
	init: bool,
	deps: HashMap<Utf8PathBuf, Vec<u8>>,
	path: Box<[Utf8PathBuf]>,
}

impl<G: Send + Sync> Trace<G> {
	fn new(task: Task<G>) -> Self {
		Self {
			task,
			init: true,
			deps: HashMap::new(),
			path: Box::new([]),
		}
	}

	fn new_with(&self, deps: HashMap<Utf8PathBuf, Vec<u8>>, path: Box<[Utf8PathBuf]>) -> Self {
		Self {
			task: self.task.clone(),
			init: false,
			deps,
			path,
		}
	}

	fn is_outdated(&self, inputs: &HashMap<Utf8PathBuf, InputItem>) -> bool {
		self.init
			|| self
				.deps
				.iter()
				.any(|dep| Some(dep.1) != inputs.get(dep.0).map(|item| &item.hash))
	}
}

fn optimize_image(buffer: &[u8]) -> Vec<u8> {
	let img = image::load_from_memory(buffer).expect("Couldn't load image");
	let dim = (img.width(), img.height());

	let mut out = Vec::new();
	let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);

	encoder
		.encode(&img.to_rgba8(), dim.0, dim.1, image::ColorType::Rgba8)
		.expect("Encoding error");

	out
}

#[derive(Debug)]
pub(crate) struct Builder {
	state: HashMap<Vec<u8>, Utf8PathBuf>,
}

impl Builder {
	pub(crate) fn new() -> Self {
		Self {
			state: HashMap::new(),
		}
	}

	/** **Pure** */
	pub(crate) fn check(&self, input: &InputItem) -> Option<Utf8PathBuf> {
		match &input.data {
			Input::Content(_) => None,
			Input::Library(_) => None,
			Input::Picture => self.state.get(&input.hash).cloned(),
			Input::Stylesheet(_) => self.state.get(&input.hash).cloned(),
			Input::Script => self.state.get(&input.hash).cloned(),
		}
	}

	/** **IO** */
	pub(crate) fn build(&mut self, input: &InputItem) -> Utf8PathBuf {
		match &input.data {
			Input::Content(_) => "".into(),
			Input::Library(_) => "".into(),
			Input::Picture => {
				let hash = crate::utils::hex(&input.hash);
				let path = Utf8Path::new("hash").join(&hash).with_extension("webp");
				let path_cache = Utf8Path::new(".cache").join(&path);

				if !path_cache.exists() {
					let buffer = fs::read(&input.file).unwrap();
					let buffer = optimize_image(&buffer);
					fs::create_dir_all(".cache/hash").unwrap();
					fs::write(&path_cache, buffer).expect("Couldn't output optimized image");
				}

				let path_root = Utf8Path::new("/").join(&path);
				let path_dist = Utf8Path::new("dist").join(&path);

				println!("IMG: {}", path_dist);
				fs::create_dir_all(path_dist.parent().unwrap_or(&path_dist)).unwrap();
				fs::copy(&path_cache, path_dist).unwrap();

				self.state.insert(input.hash.clone(), path_root.clone());
				path_root
			}
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
			Input::Script => {
				let hash = crate::utils::hex(&input.hash);
				let path = Utf8Path::new("hash").join(&hash).with_extension("js");

				let path_root = Utf8Path::new("/").join(&path);
				let path_dist = Utf8Path::new("dist").join(&path);

				println!("JS: {}", path_dist);
				fs::create_dir_all(path_dist.parent().unwrap_or(&path_dist)).unwrap();
				fs::copy(&input.file, path_dist).unwrap();

				self.state.insert(input.hash.clone(), path_root.clone());
				path_root
			}
		}
	}
}

#[derive(Debug)]
pub(crate) struct Scheduler<'a, G: Send + Sync> {
	context: &'a Context<G>,
	builder: Arc<RwLock<Builder>>,
	tracked: Vec<Trace<G>>,
	items: HashMap<Utf8PathBuf, InputItem>,
}

impl<'a, G: Send + Sync> Scheduler<'a, G> {
	pub fn new(website: &'a Website<G>, context: &'a Context<G>, items: Vec<InputItem>) -> Self {
		Self {
			context,
			builder: Arc::new(RwLock::new(Builder::new())),
			tracked: website.tasks.iter().cloned().map(Trace::new).collect(),
			items: HashMap::from_iter(items.into_iter().map(|item| (item.file.clone(), item))),
		}
	}

	pub fn build(&mut self) {
		self.tracked = mem::take(&mut self.tracked)
			.into_par_iter()
			.map(|trace| self.rebuild_trace(trace))
			.collect::<Vec<_>>();
	}

	pub fn update(&mut self, inputs: Vec<InputItem>) {
		for input in inputs {
			self.items.insert(input.file.clone(), input);
		}
	}

	pub fn build_sitemap(&self, opts: &Utf8Path) -> Box<[u8]> {
		let urls = self
			.tracked
			.iter()
			.flat_map(|x| &x.path)
			.collect::<HashSet<_>>()
			.into_iter()
			.map(|path| {
				Url::builder(opts.join(path).parent().unwrap().to_string())
					.change_frequency(ChangeFrequency::Monthly)
					.priority(0.8)
					.build()
					.expect("failed a <url> validation")
			})
			.collect::<Vec<_>>();
		let urls = UrlSet::new(urls).expect("failed a <urlset> validation");
		let mut buf = Vec::<u8>::new();
		urls.write(&mut buf).expect("failed to write XML");
		buf.into()
	}

	fn rebuild_trace(&self, trace: Trace<G>) -> Trace<G> {
		if !trace.is_outdated(&self.items) {
			return trace;
		}

		let deps = Rc::new(RefCell::new(HashMap::new()));

		let pages = trace.task.run(Sack {
			context: self.context,
			builder: self.builder.clone(),
			tracked: deps.clone(),
			items: &self.items,
		});

		// output
		for (path, data) in pages.iter() {
			let path = Utf8Path::new("dist").join(path);
			if let Some(dir) = path.parent() {
				fs::create_dir_all(dir).unwrap();
			}
			let mut file = fs::File::create(&path).unwrap();
			file.write_all(data.as_bytes()).unwrap();
			println!("HTML: {}", path);
		}

		let deps = Rc::try_unwrap(deps).unwrap();
		let deps = deps.into_inner();

		trace.new_with(deps, pages.into_iter().map(|x| x.0).collect())
	}
}
