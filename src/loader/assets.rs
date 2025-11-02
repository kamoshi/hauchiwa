use crate::{
    Globals, SiteConfig,
    loader::{File, glob::GlobRegistryTask},
    task::Handle,
};

pub fn glob_assets<G: Send + Sync + 'static, R: Clone + Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path_glob: &'static str,
    callback: impl Fn(&Globals<G>, File<Vec<u8>>) -> Result<R, anyhow::Error> + Send + Sync + 'static,
) -> Handle<super::Registry<R>> {
    site_config.add_task_opaque(GlobRegistryTask::new(
        vec![path_glob],
        vec![path_glob],
        move |ctx, file| {
            let path = file.path.clone();
            let res = callback(ctx, file).unwrap();

            Ok((path, res))
        },
    ))
}
