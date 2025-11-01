use crate::{
    SiteConfig,
    loader::{File, Registry, glob::GlobRegistryTask, parse_yaml},
    task::Handle,
};
use camino::Utf8PathBuf;
use serde::de::DeserializeOwned;

#[derive(Clone)]
pub struct Content<T: Clone> {
    pub path: Utf8PathBuf,
    pub metadata: T,
    pub content: String,
}

pub fn glob_content<G, T: Clone>(
    site_config: &mut SiteConfig<G>,
    path_glob: &'static str,
) -> Handle<Registry<Content<T>>>
where
    T: DeserializeOwned + Send + Sync + 'static,
    G: Send + Sync + 'static,
{
    site_config.add_task_opaque(GlobRegistryTask::new(
        path_glob,
        path_glob,
        move |_, file: File<Vec<u8>>| {
            let data = std::str::from_utf8(&file.metadata)?;
            let (metadata, content) = parse_yaml::<T>(data).unwrap();

            Ok((
                file.path.clone(),
                Content {
                    path: file.path,
                    metadata,
                    content,
                },
            ))
        },
    ))
}
