use std::{collections::HashSet, fs};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{BuilderError, Hash32, InputItem};

pub mod content;
pub mod generic;
#[cfg(feature = "images")]
pub mod image;
pub mod scss;
pub mod svelte;
pub mod ts;

pub(crate) trait Loadable: 'static + Send {
    fn load(&mut self);
    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> bool;
    fn items(&self) -> Vec<&InputItem>;
    fn path_base(&self) -> &'static str;
    fn remove(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool;
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
