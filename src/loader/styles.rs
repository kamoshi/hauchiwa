use camino::Utf8PathBuf;

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
                let opts = grass::Options::default().style(grass::OutputStyle::Compressed);
                let data = grass::from_path(path, &opts).unwrap();
                let hash = Hash32::hash(&data);

                (hash, data)
            },
            |rt, data| {
                let path = rt.store(data.as_bytes(), "css").unwrap();

                Style { path }
            },
        )
    })
}
