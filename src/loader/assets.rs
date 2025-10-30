use crate::{
    loader::{glob::GlobLoaderTask, File},
    task::Handle,
    SiteConfig,
};
use camino::Utf8Path;

#[derive(Clone)]
pub struct Asset {
    pub path: camino::Utf8PathBuf,
}

pub fn glob_assets<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path_base: &'static str,
    path_glob: &'static str,
) -> Handle<Vec<Asset>> {
    let task = GlobLoaderTask::new(path_base, path_glob, move |_globals, file: File<Vec<u8>>| {
        let path = file.path.strip_prefix(path_base).unwrap();
        let path = Utf8Path::new("dist").join(path);
        let dir = path.parent().unwrap();
        std::fs::create_dir_all(dir)?;
        std::fs::copy(&file.path, &path)?;
        Ok(Asset { path })
    });
    site_config.add_task_opaque(task)
}
