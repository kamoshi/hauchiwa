use crate::{
    loader::{BundleLoaderTask, File, FileLoaderTask, Runtime},
    task::Handle,
    SiteConfig,
};
use camino::Utf8Path;

#[derive(Clone)]
pub struct Style {
    pub path: camino::Utf8PathBuf,
}

pub fn glob_styles<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path_base: &'static str,
    path_glob: &'static str,
) -> Handle<Vec<Style>> {
    let task = FileLoaderTask::new(path_base, path_glob, move |_globals, file| {
        let data = grass::from_path(file.path, &grass::Options::default())?;
        let rt = Runtime {};
        let path = rt.store(data.as_bytes(), "css")?;
        Ok(Style { path })
    });
    site_config.add_task_opaque(task)
}

pub fn build_style<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    entry_point: &'static str,
    watch_glob: &'static str,
) -> Handle<Style> {
    let task = BundleLoaderTask::new(entry_point, watch_glob, move |_globals, file| {
        let data = grass::from_path(file.path, &grass::Options::default())?;
        let rt = Runtime {};
        let path = rt.store(data.as_bytes(), "css")?;
        Ok(Style { path })
    });
    site_config.add_task_opaque(task)
}
