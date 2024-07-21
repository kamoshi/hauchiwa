pub(crate) mod content;
pub(crate) mod hash;
pub(crate) mod js;
pub(crate) mod pagefind;
pub(crate) mod styles;

use std::fs;
use std::io;
use std::path::Path;
use std::rc::Rc;

use crate::gen::content::build_content;
use crate::gen::hash::build_hash;
use crate::gen::js::build_js;
use crate::gen::pagefind::build_pagefind;
use crate::gen::styles::build_css;
use crate::site::Source;
use crate::tree::{Asset, AssetKind, FileItemKind, Output, PipelineItem};
use crate::Artifacts;
use crate::{BuildContext, Website};

pub(crate) fn clean_dist() {
	println!("Cleaning dist");
	if fs::metadata("dist").is_ok() {
		fs::remove_dir_all("dist").unwrap();
	}
	fs::create_dir("dist").unwrap();
}

pub(crate) fn build_static() {
	copy_recursively(std::path::Path::new("public"), std::path::Path::new("dist")).unwrap();
}

pub(crate) fn copy_recursively(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
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

pub(crate) fn build(ctx: &BuildContext, site: &Website) -> (Vec<Rc<Output>>, Artifacts) {
	clean_dist();

	let content: Vec<Output> = site
		.sources
		.iter()
		.flat_map(Source::get)
		.map(to_bundle)
		.filter_map(Option::from)
		.collect();

	let assets: Vec<_> = content
		.iter()
		.chain(site.special.iter().map(AsRef::as_ref))
		.collect();

	let artifacts = Artifacts {
		images: build_hash(&content, ".cache".into()),
		styles: build_css(),
		javascript: build_js(&site.js, &site.dist, &site.dist_js),
	};

	build_content(ctx, &assets, &assets, &artifacts);
	build_static();
	build_pagefind(&site.dist);

	(
		content
			.into_iter()
			.map(Rc::new)
			.chain(site.special.iter().map(ToOwned::to_owned))
			.collect(),
		artifacts,
	)
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
