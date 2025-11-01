use camino::Utf8PathBuf;
use image::{EncodableLayout, io::Reader};
use std::io::Cursor;
use webp;

use crate::{
    SiteConfig,
    loader::{File, Runtime, glob::GlobRegistryTask},
    task::Handle,
};

#[derive(Clone)]
pub struct Image {
    pub path: Utf8PathBuf,
}

pub fn glob_images<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path_base: &'static str,
    path_glob: &'static str,
) -> Handle<Vec<Image>> {
    site_config.add_task_opaque(GlobRegistryTask::new(
        path_base,
        path_glob,
        move |_globals, file: File<Vec<u8>>| {
            let mut webp_bytes: Vec<u8> = Vec::new();
            let image = Reader::new(Cursor::new(&file.metadata))
                .with_guessed_format()?
                .decode()?;
            let encoder = webp::Encoder::from_image(&image).unwrap();
            let webp = encoder.encode(80.0);
            webp_bytes.extend_from_slice(webp.as_bytes());

            let rt = Runtime;
            let path = rt.store(&webp_bytes, "webp")?;
            Ok((file.path, Image { path }))
        },
    ))
}
