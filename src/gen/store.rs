//! This module provides functionality to build a hash map of optimized images from the provided
//! content. It filters out the images from the content, optimizes them, and stores them in a
//! cache. The resulting hash map contains the original paths of the images and their corresponding
//! paths in the cache.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use base64::engine::general_purpose;
use base64::Engine;
use camino::{Utf8Path, Utf8PathBuf};
use glob::GlobError;
use rayon::iter::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};
use sha2::{Digest, Sha256};

use crate::gen::copy_recursively;
use crate::tree::{AssetKind, Output, OutputKind};
use crate::utils::hex;
use crate::Website;

#[derive(Debug, Default)]
pub struct Store {
	pub images: HashMap<Utf8PathBuf, Utf8PathBuf>,
	pub styles: HashMap<String, HashedStyle>,
	pub javascript: HashMap<String, HashedScript>,
}

pub(crate) fn build_store<G: Send + Sync>(ws: &Website<G>, content: &[Output<G>]) -> Store {
	let store = Store {
		images: build_store_images(content, ".cache".into()),
		styles: build_store_styles(),
		javascript: build_js(&ws.javascript, &ws.dir_dist, &ws.dist_js),
	};

	copy_recursively(".cache", "dist/hash").unwrap();

	store
}

/// Builds a hash map of optimized images from the provided content.
///
/// This function filters out image assets from the given content, optimizes them, and stores them
/// in the specified cache directory. The resulting hash map contains the original paths of the images
/// and their corresponding paths in the cache.
///
/// # Arguments
///
/// * `content` - A slice of `Output` objects representing the content.
/// * `cache` - A reference to a `Utf8Path` representing the cache directory.
///
/// # Returns
///
/// A `HashMap` where the keys are the original paths of the images and the values are the paths to the optimized images in the cache.
pub(crate) fn build_store_images<G: Send + Sync>(
	content: &[Output<G>],
	cache: &Utf8Path,
) -> HashMap<Utf8PathBuf, Utf8PathBuf> {
	println!("Optimizing images. Cache in {}", cache);
	let now = std::time::Instant::now();

	let images: Vec<&Output<G>> = content
		.par_iter()
		.filter(|&e| match e.kind {
			OutputKind::Asset(ref a) => matches!(a.kind, AssetKind::Image),
			_ => false,
		})
		.collect();

	let hashes = hash_assets(cache, &images);
	println!("Finished optimizing. Elapsed: {:.2?}", now.elapsed());
	hashes
}

fn hash_assets<G: Send + Sync>(
	cache: &Utf8Path,
	items: &[&Output<G>],
) -> HashMap<Utf8PathBuf, Utf8PathBuf> {
	fs::create_dir_all(cache).unwrap();

	items
		.iter()
		.filter_map(|item| match item.kind {
			OutputKind::Asset(ref asset) => match asset.kind {
				AssetKind::Image => {
					let buffer = std::fs::read(asset.meta.get_path()).expect("Couldn't read file");
					let format = image::guess_format(&buffer).expect("Couldn't read format");

					if matches!(format, image::ImageFormat::Gif) {
						return None;
					}

					let path = item.path.to_owned();
					let hash = hash_image(cache, &buffer, asset.meta.get_path());
					Some((path, hash))
				}
				_ => None,
			},
			_ => None,
		})
		.collect()
}

fn optimize_image(buffer: &[u8], file: &Utf8Path, path: &Utf8Path) -> Vec<u8> {
	println!("Hashing image {} -> {}", file, path);
	let img = image::load_from_memory(buffer).expect("Couldn't load image");
	let dim = (img.width(), img.height());

	let mut out = Vec::new();
	let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);

	encoder
		.encode(&img.to_rgba8(), dim.0, dim.1, image::ColorType::Rgba8)
		.expect("Encoding error");

	out
}

fn hash_image(cache: &Utf8Path, buffer: &[u8], file: &Utf8Path) -> Utf8PathBuf {
	let hash = Sha256::digest(buffer);
	let hash = crate::utils::hex(&hash);
	let path = cache.join(&hash).with_extension("webp");

	if !path.exists() {
		let img = optimize_image(buffer, file, &path);
		fs::write(path, img).expect("Couldn't output optimized image");
	}

	Utf8Path::new("/")
		.join("hash")
		.join(hash)
		.with_extension("webp")
}

#[derive(Debug)]
pub struct HashedStyle {
	pub name: String,
	pub path: Utf8PathBuf,
	pub sri: String,
}

pub(crate) fn build_store_styles() -> HashMap<String, HashedStyle> {
	println!("Compiling styles...");
	let now = Instant::now();
	let styles = compile_styles();
	println!("Compiled styles in {:.2?}", now.elapsed());
	styles
}

fn compile_styles() -> HashMap<String, HashedStyle> {
	glob::glob("styles/**/[!_]*.scss")
		.expect("Failed to read glob pattern")
		.par_bridge()
		.filter_map(compile)
		.map(|e| (e.name.clone(), e))
		.collect()
}

fn compile(entry: Result<PathBuf, GlobError>) -> Option<HashedStyle> {
	match entry {
		Ok(path) => {
			let name = path.file_stem().unwrap().to_string_lossy();
			let opts = grass::Options::default().style(grass::OutputStyle::Compressed);
			let code = grass::from_path(&path, &opts).unwrap();
			let hash = Sha256::digest(&code);
			let hash_hex = hex(&hash);
			let hash_sri = format!("sha256-{}", general_purpose::STANDARD.encode(hash));

			let path_cache = Utf8Path::new(".cache")
				.join(&hash_hex)
				.with_extension("css");

			let path_store = Utf8Path::new("/")
				.join("hash")
				.join(&hash_hex)
				.with_extension("css");

			fs::write(path_cache, code).unwrap();

			Some(HashedStyle {
				name: name.to_string(),
				path: path_store,
				sri: hash_sri,
			})
		}
		Err(e) => {
			eprintln!("{:?}", e);
			None
		}
	}
}

#[derive(Debug)]
pub struct HashedScript {
	pub name: String,
	pub path: Utf8PathBuf,
	pub sri: String,
}

pub(crate) fn build_js(
	js: &HashMap<&str, &str>,
	out: &Utf8Path,
	dir: &Utf8Path,
) -> HashMap<String, HashedScript> {
	let mut cmd = Command::new("esbuild");

	for (alias, path) in js.iter() {
		cmd.arg(format!("{}={}", alias, path));
	}

	let res = cmd
		.arg("--format=esm")
		.arg("--bundle")
		.arg("--splitting")
		.arg("--minify")
		.arg(format!("--outdir={}", out.join(dir)))
		.output()
		.unwrap();

	let stderr = String::from_utf8(res.stderr).unwrap();
	println!("{}", stderr);

	let mut hashed = HashMap::new();

	for key in js.keys() {
		let path = out.join(dir).join(key).with_extension("js");
		let data = std::fs::read(&path).expect("Couldn't read file");
		let hash = Sha256::digest(&data);
		let hash_sri = format!("sha256-{}", general_purpose::STANDARD.encode(hash));

		hashed.insert(
			key.to_string(),
			HashedScript {
				name: key.to_string(),
				path: Utf8Path::new("/").join(dir).join(key).with_extension("js"),
				sri: hash_sri,
			},
		);
	}

	hashed
}
