use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;

use camino::{Utf8Path, Utf8PathBuf};

use crate::site::Source;
use crate::tree::{Asset, AssetKind, FileItemKind, Output, OutputKind, PipelineItem, Sack, Virtual};
use crate::BuildContext;

pub(crate) fn clean_dist() {
	println!("Cleaning dist");
	if fs::metadata("dist").is_ok() {
		fs::remove_dir_all("dist").unwrap();
	}
	fs::create_dir("dist").unwrap();
}

pub(crate) fn build_hash(
	content: &[Output],
	cache: &Utf8Path,
) -> HashMap<Utf8PathBuf, Utf8PathBuf> {
	println!("Optimizing images. Cache in {}", cache);
	let now = std::time::Instant::now();

	let images: Vec<&Output> = content
		.iter()
		.filter(|&e| match e.kind {
			OutputKind::Asset(ref a) => matches!(a.kind, AssetKind::Image),
			_ => false,
		})
		.collect();

	let hashes = crate::hash::hash_assets(cache, &images);
	copy_recursively(cache, Path::new("dist/hash")).unwrap();
	println!("Finished optimizing. Elapsed: {:.2?}", now.elapsed());
	hashes
}

pub(crate) fn build_styles() {
	let css = grass::from_path("styles/styles.scss", &grass::Options::default()).unwrap();
	fs::write("dist/styles.css", css).unwrap();
}

pub(crate) fn build_content(
	ctx: &BuildContext,
	pending: &[&Output],
	hole: &[&Output],
	hash: Option<HashMap<Utf8PathBuf, Utf8PathBuf>>,
) {
	let now = std::time::Instant::now();
	render_all(ctx, pending, hole, hash);
	println!("Elapsed: {:.2?}", now.elapsed());
}

pub(crate) fn build_static() {
	copy_recursively(std::path::Path::new("public"), std::path::Path::new("dist")).unwrap();
}

pub(crate) fn build_pagefind() {
	let res = Command::new("pagefind")
		.args(["--site", "dist"])
		.output()
		.unwrap();

	println!("{}", String::from_utf8(res.stdout).unwrap());
}

pub(crate) fn build_js() {
	let res = Command::new("esbuild")
		.arg("js/vanilla/reveal.js")
		.arg("js/vanilla/photos.ts")
		.arg("js/search/dist/search.js")
		.arg("--format=esm")
		.arg("--bundle")
		.arg("--splitting")
		.arg("--minify")
		.arg("--outdir=dist/js/")
		.output()
		.unwrap();

	println!("{}", String::from_utf8(res.stderr).unwrap());
}

fn copy_recursively(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
	fs::create_dir_all(&dst)?;
	for entry in fs::read_dir(src)? {
		let entry = entry?;
		let filetype = entry.file_type()?;
		if filetype.is_dir() {
			copy_recursively(entry.path(), dst.as_ref().join(entry.file_name()))?;
		} else {
			fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
		}
	}
	Ok(())
}

fn render_all(
	ctx: &BuildContext,
	pending: &[&Output],
	hole: &[&Output],
	hash: Option<HashMap<Utf8PathBuf, Utf8PathBuf>>,
) {
	pending
		.iter()
		.map(|item| {
			let file = match &item.kind {
				OutputKind::Asset(a) => Some(&a.meta.path),
				OutputKind::Virtual(_) => None,
			};

			render(
				item,
				Sack {
					ctx,
					hole,
					path: &item.path,
					file,
					hash: hash.clone(),
				},
			)
		})
		.collect()
}

fn render(item: &Output, sack: Sack) {
	let dist = Utf8Path::new("dist");
	let o = dist.join(&item.path);
	fs::create_dir_all(o.parent().unwrap()).unwrap();

	match item.kind {
		OutputKind::Asset(ref real) => {
			let i = &real.meta.path;

			match &real.kind {
				AssetKind::Html(closure) => {
					let mut file = File::create(&o).unwrap();
					file.write_all(closure(&sack).as_bytes()).unwrap();
					println!("HTML: {} -> {}", i, o);
				}
				AssetKind::Bibtex(_) => (),
				AssetKind::Image => {
					fs::create_dir_all(o.parent().unwrap()).unwrap();
					fs::copy(i, &o).unwrap();
					println!("Image: {} -> {}", i, o);
				}
			}
		}
		OutputKind::Virtual(Virtual(ref closure)) => {
			let mut file = File::create(&o).unwrap();
			file.write_all(closure(&sack).as_bytes()).unwrap();
			println!("Virtual: -> {}", o);
		}
	}
}

pub(crate) fn build(
	ctx: &BuildContext,
	sources: &[Source],
	special: &[Rc<Output>],
) -> Vec<Rc<Output>> {
	crate::build::clean_dist();

	let content: Vec<Output> = sources
		.iter()
		.flat_map(Source::get)
		.map(to_bundle)
		.filter_map(Option::from)
		.collect();

	let assets: Vec<_> = content
		.iter()
		.chain(special.iter().map(AsRef::as_ref))
		.collect();

	let hashes = crate::build::build_hash(&content, ".cache".into());
	crate::build::build_content(ctx, &assets, &assets, Some(hashes));
	crate::build::build_static();
	crate::build::build_styles();
	crate::build::build_pagefind();
	crate::build::build_js();

	content
		.into_iter()
		.map(Rc::new)
		.chain(special.iter().map(ToOwned::to_owned))
		.collect()
}

fn to_bundle(item: PipelineItem) -> PipelineItem {
	let meta = match item {
		PipelineItem::Skip(meta) if matches!(meta.kind, FileItemKind::Bundle) => meta,
		_ => return item,
	};

	let path = meta.path.strip_prefix("content").unwrap().to_owned();

	match meta.path.extension() {
		// any image
		Some("jpg" | "png" | "gif") => Output {
			kind: Asset {
				kind: AssetKind::Image,
				meta,
			}
			.into(),
			path,
			link: None,
		}
		.into(),
		// bibliography
		Some("bib") => {
			let data = fs::read_to_string(&meta.path).unwrap();
			let data = hayagriva::io::from_biblatex_str(&data).unwrap();

			Output {
				kind: Asset {
					kind: AssetKind::Bibtex(data),
					meta,
				}
				.into(),
				path,
				link: None,
			}
			.into()
		}
		_ => meta.into(),
	}
}
