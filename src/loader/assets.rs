use std::fs;

use crate::{
    Hash32, Loader,
    loader::{Runtime, generic::LoaderGeneric},
};

pub fn glob_assets<T>(
    path_base: &'static str,
    path_glob: &'static str,
    func: fn(Runtime, Vec<u8>) -> anyhow::Result<T>,
) -> Loader
where
    T: Send + Sync + 'static,
{
    Loader::with(move |_| {
        LoaderGeneric::new(
            path_base,
            path_glob,
            |path| {
                let data = fs::read(path)?;
                let hash = Hash32::hash(&data);

                Ok((hash, data))
            },
            func,
        )
    })
}
