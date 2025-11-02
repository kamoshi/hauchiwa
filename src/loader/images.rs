use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

use std::sync::{Arc, OnceLock};

use crate::{
    error::{BuildError, LazyAssetError},
    loader::{glob::GlobRegistryTask, File, Registry},
    task::Handle,
    Hash32, SiteConfig,
};

type LazyAsset<T> = Arc<OnceLock<Result<T, LazyAssetError>>>;

#[derive(Clone)]
pub struct Image {
    path: Utf8PathBuf,
    hash: Hash32,
    asset: LazyAsset<Utf8PathBuf>,
}

impl Image {
    pub fn path(&self) -> Result<Utf8PathBuf, LazyAssetError> {
        self.asset
            .get_or_init(|| build_image(self.hash, &self.path).map_err(LazyAssetError::new))
            .clone()
    }
}

pub fn glob_images<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path_glob: &'static str,
) -> Handle<Registry<Image>> {
    site_config.add_task_opaque(GlobRegistryTask::new(
        path_glob,
        path_glob,
        move |_, file: File<Vec<u8>>| {
            let hash = Hash32::hash(&file.metadata);
            let asset = Arc::new(OnceLock::new());
            let path = file.path;
            Ok((path.clone(), Image { path, hash, asset }))
        },
    ))
}

fn process_image(buffer: &[u8]) -> image::ImageResult<Vec<u8>> {
    let img = image::load_from_memory(buffer)?;
    let w = img.width();
    let h = img.height();

    let mut out = Vec::new();
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);

    encoder.encode(&img.to_rgba8(), w, h, image::ExtendedColorType::Rgba8)?;

    Ok(out)
}

fn build_image(hash: Hash32, file: &Utf8Path) -> Result<Utf8PathBuf, BuildError> {
    let hash = hash.to_hex();
    let path_root = Utf8Path::new("/hash/img/")
        .join(&hash)
        .with_extension("webp");
    let path_hash = Utf8Path::new(".cache/hash/img/")
        .join(&hash)
        .with_extension("webp");
    let path_dist = Utf8Path::new("dist/hash/img/")
        .join(&hash)
        .with_extension("webp");

    // If this hash exists it means the work is already done.
    if !path_hash.exists() {
        let buffer = fs::read(file)?;
        let buffer = process_image(&buffer) //
            .map_err(|err| BuildError::Other(err.into()))?;

        fs::create_dir_all(".cache/hash/img/")?;
        fs::write(&path_hash, buffer)?;
    }

    let dir = path_dist.parent().unwrap_or(&path_dist);
    fs::create_dir_all(dir)?;
    fs::copy(&path_hash, &path_dist)?;

    Ok(path_root)
}
