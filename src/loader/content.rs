use crate::{
    SiteConfig,
    error::HauchiwaError,
    loader::{File, Registry, glob::GlobRegistryTask, parse_yaml},
    task::Handle,
};
use camino::Utf8PathBuf;
use serde::de::DeserializeOwned;

#[derive(Clone)]
pub struct Content<T> {
    pub path: Utf8PathBuf,
    pub metadata: T,
    pub content: String,
}

pub fn glob_content<G, R>(
    site_config: &mut SiteConfig<G>,
    path_glob: &'static str,
) -> Result<Handle<Registry<Content<R>>>, HauchiwaError>
where
    G: Send + Sync + 'static,
    R: DeserializeOwned + Send + Sync + 'static,
{
    Ok(site_config.add_task_opaque(GlobRegistryTask::new(
        vec![path_glob],
        vec![path_glob],
        move |_, _, file: File<Vec<u8>>| {
            let data = std::str::from_utf8(&file.metadata)?;
            let (metadata, content) = parse_yaml::<R>(data)?;

            Ok((
                file.path.clone(),
                Content {
                    path: file.path,
                    metadata,
                    content,
                },
            ))
        },
    )?))
}
