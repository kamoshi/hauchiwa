//! This module provides functionality to build a hash map of optimized images from the provided
//! content. It filters out the images from the content, optimizes them, and stores them in a
//! cache. The resulting hash map contains the original paths of the images and their corresponding
//! paths in the cache.

use std::collections::HashMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::gen::copy_recursively;
use crate::tree::{AssetKind, Output, OutputKind};

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

	let hashes = hash_assets(cache, &images);
	copy_recursively(cache, "dist/hash").unwrap();
	println!("Finished optimizing. Elapsed: {:.2?}", now.elapsed());
	hashes
}

fn hash_assets(cache: &Utf8Path, items: &[&Output]) -> HashMap<Utf8PathBuf, Utf8PathBuf> {
	fs::create_dir_all(cache).unwrap();

	items
		.par_iter()
		.filter_map(|item| match item.kind {
			OutputKind::Asset(ref asset) => match asset.kind {
				AssetKind::Image => {
					let buffer = std::fs::read(&asset.meta.path).expect("Couldn't read file");
					let format = image::guess_format(&buffer).expect("Couldn't read format");

					if matches!(format, image::ImageFormat::Gif) {
						return None;
					}

					let path = item.path.to_owned();
					let hash = hash_image(cache, &buffer, &asset.meta.path);
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
	let hash = sha256::digest(buffer);
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
