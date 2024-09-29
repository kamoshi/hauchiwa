use std::fs;

use camino::Utf8Path;

use crate::builder::{Input, InputItem};
use crate::{Collection, Context, Website};

pub struct QueryContent<'a, D> {
	pub file: &'a Utf8Path,
	pub slug: &'a Utf8Path,
	pub meta: &'a D,
	pub content: &'a str,
}

/// This struct allows for querying the website hierarchy. It is passed to each rendered website
/// page, so that it can easily access the website metadata.
pub struct Sack<'a, G: Send + Sync> {
	/// Global `Context` for the current build.
	context: &'a Context<G>,
	/// Every single input.
	items: &'a [&'a InputItem],
	// /// Current path for the page being rendered
	// path: &'a Utf8Path,
	// /// Processed artifacts (styles, scripts, etc.)
	// store: &'a Store,
	// /// Original file location for this page
	// file: Option<&'a Utf8Path>,
}

impl<'a, G: Send + Sync> Sack<'a, G> {
	/// Retrieve global context
	pub fn get_context(&self) -> &Context<G> {
		self.context
	}

	pub fn get_content<D: 'static>(&self, pattern: &str) -> Option<QueryContent<'_, D>> {
		let pattern = glob::Pattern::new(pattern).expect("Bad glob pattern");

		self.items
			.iter()
			.filter(|item| pattern.matches_path(item.slug.as_ref()))
			.filter_map(|item| {
				let (meta, content) = match &item.data {
					Input::Content(input_content) => {
						let meta = input_content.meta.downcast_ref::<D>()?;
						let data = input_content.content.as_str();
						Some((meta, data))
					}
					_ => None,
				}?;

				Some(QueryContent {
					file: &item.file,
					slug: &item.slug,
					meta,
					content,
				})
			})
			.next()
	}

	/// Retrieve many possible content items.
	pub fn get_content_list<D: 'static>(&self, pattern: &str) -> Vec<QueryContent<'_, D>> {
		let pattern = glob::Pattern::new(pattern).expect("Bad glob pattern");

		self.items
			.iter()
			.filter(|item| pattern.matches_path(item.slug.as_ref()))
			.filter_map(|item| {
				let (meta, content) = match &item.data {
					Input::Content(input_content) => {
						let meta = input_content.meta.downcast_ref::<D>()?;
						let data = input_content.content.as_str();
						Some((meta, data))
					}
					_ => None,
				}?;

				Some(QueryContent {
					file: &item.file,
					slug: &item.slug,
					meta,
					content,
				})
			})
			.collect()
	}

	/// Get compiled CSS style by alias.
	pub fn get_styles(&self, alias: &str) -> Option<&Utf8Path> {
		// todo!()
		Some("".into())
	}

	/// Get optimized image path by original path.
	pub fn get_picture(&self, path: &Utf8Path) -> Option<&Utf8Path> {
		// self.store.images.get(alias).map(AsRef::as_ref)
		// todo!()
		Some("".into())
	}

	pub fn get_script(&self, alias: &str) -> Option<&Utf8Path> {
		// todo!()
		Some("".into())
	}

	// pub fn get_library(&self) -> Option<&Library> {
	// 	let glob = format!("{}/*.bib", self.path.parent()?);
	// 	let glob = glob::Pattern::new(&glob).expect("Bad glob pattern");
	// 	let opts = glob::MatchOptions {
	// 		case_sensitive: true,
	// 		require_literal_separator: true,
	// 		require_literal_leading_dot: false,
	// 	};

	// 	self.hole
	// 		.iter()
	// 		.filter(|item| glob.matches_path_with(item.path.as_ref(), opts))
	// 		.filter_map(|asset| match asset.kind {
	// 			OutputKind::Asset(ref real) => Some(real),
	// 			_ => None,
	// 		})
	// 		.find_map(|asset| match asset.kind {
	// 			AssetKind::Bibtex(ref lib) => Some(lib),
	// 			_ => None,
	// 		})
	// }

	// /// Get the path for original file location
	// pub fn get_file(&self) -> Option<&'a Utf8Path> {
	// 	self.file
	// }

	// pub fn get_import_map(&self) -> String {
	// 	let ok = self
	// 		.store
	// 		.javascript
	// 		.iter()
	// 		.map(|(k, v)| (k.clone(), v.path.clone()))
	// 		.collect();
	// 	let map = ImportMap { imports: &ok };

	// 	serde_json::to_string(&map).unwrap()
	// }
}

pub(crate) fn build<G: Send + Sync + 'static>(website: &Website<G>, context: &Context<G>) {
	clean_dist();

	let items: Vec<_> = website
		.collections
		.iter()
		.flat_map(Collection::load)
		.collect();

	// let store = build_store(website, &content);

	let items_ptr = items.iter().collect::<Vec<_>>();

	for task in website.tasks.iter() {
		task.run(Sack {
			context,
			items: &items_ptr,
		});
	}

	// build_static();
	// build_pagefind(&website.dir_dist);

	// (
	// 	content
	// 		.into_iter()
	// 		.map(Rc::new)
	// 		.chain(website.special.iter().map(ToOwned::to_owned))
	// 		.collect(),
	// 	store,
	// )
}

pub(crate) fn clean_dist() {
	println!("Cleaning dist");
	if fs::metadata("dist").is_ok() {
		fs::remove_dir_all("dist").unwrap();
	}
	fs::create_dir("dist").unwrap();
}

// pub(crate) fn build_static() {
// 	copy_recursively(std::path::Path::new("public"), std::path::Path::new("dist")).unwrap();
// }

// pub(crate) fn copy_recursively(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
// 	fs::create_dir_all(&dst)?;
// 	for entry in fs::read_dir(src)? {
// 		let entry = entry?;
// 		let filetype = entry.file_type()?;
// 		if filetype.is_dir() {
// 			copy_recursively(entry.path(), dst.as_ref().join(entry.file_name()))?;
// 		} else {
// 			fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
// 		}
// 	}
// 	Ok(())
// }

// fn build_content<G: Send + Sync>(
// 	ctx: &Context<G>,
// 	pending: &[&Output<G>],
// 	hole: &[&Output<G>],
// ) {
// 	let now = std::time::Instant::now();
// 	render_all(ctx, store, pending, hole);
// 	println!("Elapsed: {:.2?}", now.elapsed());
// }

// fn render_all<G: Send + Sync>(
// 	ctx: &Context<G>,
// 	store: &Store,
// 	pending: &[&Output<G>],
// 	hole: &[&Output<G>],
// ) {
// 	pending
// 		.iter()
// 		.map(|&item| {
// 			let file = match &item.kind {
// 				OutputKind::Asset(a) => Some(a.meta.get_path()),
// 				OutputKind::Virtual(_) => None,
// 			};

// 			render(
// 				item,
// 				Sack {
// 					ctx,
// 					store,
// 					hole,
// 					path: &item.path,
// 					file,
// 				},
// 			)
// 		})
// 		.collect()
// }

// fn render<G: Send + Sync>(item: &Output<G>, sack: Sack<G>) {
// 	let dist = Utf8Path::new("dist");
// 	let o = dist.join(&item.path);
// 	fs::create_dir_all(o.parent().unwrap()).unwrap();

// 	match item.kind {
// 		OutputKind::Asset(ref real) => {
// 			let fs_path = real.meta.get_path();

// 			match &real.kind {
// 				AssetKind::Html(DeferredHtml { lazy, .. }) => {
// 					let mut file = File::create(&o).unwrap();
// 					file.write_all(lazy(&sack).as_bytes()).unwrap();
// 					println!("HTML: {} -> {}", fs_path, o);
// 				}
// 				AssetKind::Bibtex(_) => (),
// 				AssetKind::Image => {
// 					fs::create_dir_all(o.parent().unwrap()).unwrap();
// 					fs::copy(fs_path, &o).unwrap();
// 					println!("Image: {} -> {}", fs_path, o);
// 				}
// 			}
// 		}
// 		OutputKind::Virtual(Virtual(ref closure)) => {
// 			let mut file = File::create(&o).unwrap();
// 			file.write_all(closure(&sack).as_bytes()).unwrap();
// 			println!("Virtual: -> {}", o);
// 		}
// 	}
// }
