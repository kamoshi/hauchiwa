pub(crate) mod content;
pub(crate) mod pagefind;
pub(crate) mod store;

use std::fs;
use std::io;
use std::path::Path;
use std::rc::Rc;

use crate::collection::Collection;
use crate::gen::content::build_content;
use crate::gen::pagefind::build_pagefind;
use crate::gen::store::{build_store, Store};
use crate::tree::FileItem;
use crate::tree::{Asset, AssetKind, Output, PipelineItem};
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

pub(crate) fn build(ctx: &BuildContext, ws: &Website) -> (Vec<Rc<Output>>, Store) {
	clean_dist();

	let content: Vec<Output> = ws
		.loaders
		.iter()
		.flat_map(Collection::load)
		.map(|x| match &x {
			FileItem::Index(index) => index.clone().process(),
			FileItem::Bundle(_) => PipelineItem::Skip(x),
		})
		.map(to_bundle)
		.filter_map(Option::from)
		.collect();

	let assets: Vec<_> = content
		.iter()
		.chain(ws.special.iter().map(AsRef::as_ref))
		.collect();

	let store = build_store(ws, &content);

	build_content(ctx, &store, &assets, &assets);
	build_static();
	build_pagefind(&ws.dist);

	(
		content
			.into_iter()
			.map(Rc::new)
			.chain(ws.special.iter().map(ToOwned::to_owned))
			.collect(),
		store,
	)
}

fn to_bundle(item: PipelineItem) -> PipelineItem {
	let meta = match item {
		PipelineItem::Skip(FileItem::Bundle(bundle)) => bundle,
		_ => return item,
	};

	let path = meta.path.strip_prefix("content").unwrap().to_owned();

	match meta.path.extension() {
		// any image
		Some("jpg" | "png" | "gif") => Output {
			kind: Asset {
				kind: AssetKind::Image,
				meta: FileItem::Bundle(meta),
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
					meta: FileItem::Bundle(meta),
				}
				.into(),
				path,
				link: None,
			}
			.into()
		}
		_ => FileItem::Bundle(meta).into(),
	}
}
