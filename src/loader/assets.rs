use crate::{
    loader::{File, glob::GlobRegistryTask, Registry},
    task::Handle,
    Globals, SiteConfig,
};

pub fn glob_assets<
    G: Send + Sync + 'static,
    R: Clone + Send + Sync + 'static,
    F: Fn(&Globals<G>, File<Vec<u8>>) -> anyhow::Result<R> + Send + Sync + 'static,
>(
    config: &mut SiteConfig<G>,
    glob: &'static str,
    callback: F,
) -> Handle<Registry<R>>
where
    R: 'static,
{
    let task = GlobRegistryTask::new(vec![glob], vec![glob], move |globals, file| {
        let path = file.path.clone();
        let result = callback(globals, file)?;
        Ok((path, result))
    });

    config.add_task_opaque(task)
}
