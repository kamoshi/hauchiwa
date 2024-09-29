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
use crate::{Context, Website};

pub(crate) fn build<G: Send + Sync + 'static>(
	website: &Website<G>,
	context: &Context<G>,
) -> (Vec<Rc<Output<G>>>, Store) {
	clean_dist();

	let content: Vec<_> = website
		.collections
		.iter()
		.flat_map(Collection::load)
		.collect();

	let assets: Vec<_> = content
		.iter()
		.chain(website.special.iter().map(AsRef::as_ref))
		.collect();

	let store = build_store(website, &content);

	build_content(context, &store, &assets, &assets);
	build_static();
	build_pagefind(&website.dir_dist);

	(
		content
			.into_iter()
			.map(Rc::new)
			.chain(website.special.iter().map(ToOwned::to_owned))
			.collect(),
		store,
	)
}

fn to_bundle<G: Send + Sync>(item: PipelineItem<G>) -> PipelineItem<G> {
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
			}
			.into()
		}
		_ => FileItem::Bundle(meta).into(),
	}
}
