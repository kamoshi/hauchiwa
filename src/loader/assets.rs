use crate::{
    loader::{File, FileLoaderTask},
    task::Handle,
    SiteConfig,
};

pub fn glob_assets<R>(
    site_config: &mut SiteConfig,
    path_base: &'static str,
    path_glob: &'static str,
    callback: fn(File<Vec<u8>>) -> anyhow::Result<R>,
) -> Handle<Vec<R>>
where
    R: Clone + Send + Sync + 'static,
{
    let task = FileLoaderTask::new(path_base, path_glob, callback);
    site_config.add_task_boxed(Box::new(task))
}
