use std::fs::{self, File};
use std::io::Write;

use camino::Utf8Path;

use crate::gen::store::Store;
use crate::tree::{AssetKind, DeferredHtml, Output, OutputKind, Virtual};
use crate::{BuildContext, Sack};

pub(crate) fn build_content(
	ctx: &BuildContext,
	store: &Store,
	pending: &[&Output],
	hole: &[&Output],
) {
	let now = std::time::Instant::now();
	render_all(ctx, store, pending, hole);
	println!("Elapsed: {:.2?}", now.elapsed());
}

fn render_all(ctx: &BuildContext, store: &Store, pending: &[&Output], hole: &[&Output]) {
	pending
		.iter()
		.map(|&item| {
			let file = match &item.kind {
				OutputKind::Asset(a) => Some(a.meta.get_path()),
				OutputKind::Virtual(_) => None,
			};

			render(
				item,
				Sack {
					ctx,
					store,
					hole,
					path: &item.path,
					file,
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
			let fs_path = real.meta.get_path();

			match &real.kind {
				AssetKind::Html(DeferredHtml { lazy, .. }) => {
					let mut file = File::create(&o).unwrap();
					file.write_all(lazy(&sack).as_bytes()).unwrap();
					println!("HTML: {} -> {}", fs_path, o);
				}
				AssetKind::Bibtex(_) => (),
				AssetKind::Image => {
					fs::create_dir_all(o.parent().unwrap()).unwrap();
					fs::copy(fs_path, &o).unwrap();
					println!("Image: {} -> {}", fs_path, o);
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
