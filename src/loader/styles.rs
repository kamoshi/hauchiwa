use camino::Utf8PathBuf;
use grass::{Options, OutputStyle, from_path};

use crate::{Hash32, Loader, loader::generic::LoaderGenericMultifile};

pub struct Style {
    pub path: Utf8PathBuf,
}

pub fn glob_styles(path_base: &'static str, path_glob: &'static str) -> Loader {
    Loader::with(move |_| {
        LoaderGenericMultifile::new(
            path_base,
            path_glob,
            |path| {
                let opts = Options::default().style(OutputStyle::Compressed);
                let data = from_path(path, &opts)?;
                let hash = Hash32::hash(&data);

                Ok((hash, data))
            },
            |rt, data| {
                let path = rt.store(data.as_bytes(), "css")?;

                Ok(Style { path })
            },
        )
    })
}
