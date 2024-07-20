use std::{collections::HashMap, fs};

use camino::{Utf8Path, Utf8PathBuf};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::tree::{AssetKind, Output, OutputKind};

pub(crate) fn hash_assets(
	cache: &Utf8Path,
	items: &[&Output],
) -> HashMap<Utf8PathBuf, Utf8PathBuf> {
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
