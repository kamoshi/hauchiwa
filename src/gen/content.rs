use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;

use camino::{Utf8Path, Utf8PathBuf};

use crate::tree::{AssetKind, Output, OutputKind, Virtual};
use crate::{Artifacts, BuildContext, Sack};

pub(crate) fn build_content(
	ctx: &BuildContext,
	pending: &[&Output],
	hole: &[&Output],
	artifacts: &Artifacts,
) {
	let now = std::time::Instant::now();
	render_all(ctx, pending, hole, artifacts);
	println!("Elapsed: {:.2?}", now.elapsed());
}

fn render_all(
	ctx: &BuildContext,
	pending: &[&Output],
	hole: &[&Output],
	artifacts: &Artifacts,
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
					artifacts: artifacts.clone(),
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
