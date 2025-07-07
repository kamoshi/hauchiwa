use camino::Utf8PathBuf;

use crate::{
    Hash32,
    plugin::{Loadable, generic::LoaderGenericMultifile},
};

pub struct Stylesheet {
    pub path: Utf8PathBuf,
}

pub(crate) fn new_loader_scss(path_base: &'static str, path_glob: &'static str) -> impl Loadable {
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

            Stylesheet { path }
        },
    )
}
