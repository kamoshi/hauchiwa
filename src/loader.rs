mod assets;
mod content;
mod generic;
#[cfg(feature = "images")]
mod images;
mod script;
#[cfg(feature = "styles")]
mod styles;
mod svelte;

use std::{collections::HashSet, fs, sync::Arc};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{BuilderError, GitRepo, Hash32, Item};

pub use assets::glob_assets;
pub use content::{Content, glob_content};
#[cfg(feature = "images")]
pub use images::{Image, glob_images};
pub use script::{Script, glob_scripts};
#[cfg(feature = "styles")]
pub use styles::{Style, glob_styles};
pub use svelte::{Svelte, glob_svelte};

pub(crate) trait Loadable: 'static + Send {
    fn load(&mut self);
    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> bool;
    fn items(&self) -> Vec<&Item>;
    fn path_base(&self) -> &'static str;
    fn remove(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool;
}

impl Loadable for Box<dyn Loadable> {
    #[inline]
    fn load(&mut self) {
        (**self).load()
    }

    #[inline]
    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> bool {
        (**self).reload(set)
    }

    #[inline]
    fn items(&self) -> Vec<&Item> {
        (**self).items()
    }

    #[inline]
    fn path_base(&self) -> &'static str {
        (**self).path_base()
    }

    #[inline]
    fn remove(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool {
        (**self).remove(obsolete)
    }
}

pub struct Loader(Box<dyn Fn(LoaderOpts) -> Box<dyn Loadable>>);

pub struct LoaderOpts {
    pub repo: Arc<GitRepo>,
}

impl Loader {
    #[inline]
    pub(crate) fn with<F, R>(f: F) -> Self
    where
        F: Fn(LoaderOpts) -> R + 'static,
        R: Loadable,
    {
        Self(Box::new(move |init| Box::new(f(init))))
    }

    #[inline]
    pub(crate) fn init(&self, opts: LoaderOpts) -> Box<dyn Loadable> {
        (self.0)(opts)
    }
}

#[derive(Clone)]
pub struct Runtime;

impl Runtime {
    pub fn store(&self, data: &[u8], ext: &str) -> Result<Utf8PathBuf, BuilderError> {
        let hash = Hash32::hash(data);
        let hash = hash.to_hex();

        let path_temp = Utf8Path::new(".cache/hash").join(&hash);
        let path_dist = Utf8Path::new("dist/hash").join(&hash).with_extension(ext);
        let path_root = Utf8Path::new("/hash/").join(&hash).with_extension(ext);

        if !path_temp.exists() {
            fs::create_dir_all(".cache/hash")
                .map_err(|e| BuilderError::CreateDirError(".cache/hash".into(), e))?;
            fs::write(&path_temp, data)
                .map_err(|e| BuilderError::FileWriteError(path_temp.clone(), e))?;
        }

        let dir = path_dist.parent().unwrap_or(&path_dist);
        fs::create_dir_all(dir) //
            .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
        fs::copy(&path_temp, &path_dist)
            .map_err(|e| BuilderError::FileCopyError(path_temp.clone(), path_dist.clone(), e))?;

        Ok(path_root)
    }
}
