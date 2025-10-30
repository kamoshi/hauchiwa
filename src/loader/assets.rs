use crate::{
    loader::{File, FileLoaderTask},
    task::Handle,
    Globals, SiteConfig,
};

pub fn glob_assets<G, R>(
    site_config: &mut SiteConfig<G>,
    path_base: &'static str,
    path_glob: &'static str,
    callback: fn(&Globals<G>, File<Vec<u8>>) -> anyhow::Result<R>,
) -> Handle<Vec<R>>
where
    G: Send + Sync + 'static,
    R: Clone + Send + Sync + 'static,
{
    let task = FileLoaderTask::new(path_base, path_glob, callback);
    site_config.add_task_opaque(task)
}
