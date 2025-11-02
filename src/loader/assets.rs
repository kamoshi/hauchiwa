use crate::{
    loader::{File, Registry, glob::GlobRegistryTask},
    task::Handle,
    SiteConfig, Globals,
};

pub fn glob_assets<G: Send + Sync + 'static, R: Clone + Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path_glob: &'static str,
    callback: impl Fn(&Globals<G>, File<Vec<u8>>) -> Result<R, anyhow::Error> + Send + Sync + 'static,
) -> Handle<Registry<R>> {
    site_config.add_task_opaque(GlobRegistryTask::new(
        path_glob,
        path_glob,
        move |globals, file| {
            let path = file.path.clone();
            let res = callback(globals, file)?;

            Ok((path, res))
        },
    ))
}
