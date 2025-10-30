use crate::{
    loader::{glob::GlobLoaderTask, Runtime},
    task::Handle,
    SiteConfig,
};
use camino::{Utf8Path, Utf8PathBuf};
use std::collections::HashMap;

#[derive(Clone)]
pub struct Style {
    pub path: camino::Utf8PathBuf,
}

#[derive(Clone)]
pub struct Styles {
    map: HashMap<camino::Utf8PathBuf, Style>,
}

impl Styles {
    pub fn get(&self, path: impl AsRef<Utf8Path>) -> Option<&Style> {
        self.map.get(path.as_ref())
    }
}

pub fn build_styles<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    entry_point_glob: &'static str,
    watch_glob: &'static str,
) -> Handle<Styles> {
    let styles_vec_handle: Handle<Vec<(Utf8PathBuf, Style)>> = {
        let task = GlobLoaderTask::new(entry_point_glob, watch_glob, move |_globals, file| {
            let data = grass::from_path(&file.path, &grass::Options::default())?;
            let rt = Runtime {};
            let path = rt.store(data.as_bytes(), "css")?;
            Ok((file.path, Style { path }))
        });
        site_config.add_task_opaque(task)
    };

    site_config.add_task(
        (styles_vec_handle,),
        |_, (styles_vec,): (&Vec<(Utf8PathBuf, Style)>,)| Styles {
            map: styles_vec.iter().cloned().collect(),
        },
    )
}
