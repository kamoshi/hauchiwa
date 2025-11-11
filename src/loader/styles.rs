use crate::{
    SiteConfig,
    error::HauchiwaError,
    loader::{Runtime, glob::GlobRegistryTask},
    task::Handle,
};

#[derive(Debug, Clone)]
pub struct CSS {
    pub path: camino::Utf8PathBuf,
}

pub fn build_styles<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    glob_entry: &'static str,
    glob_watch: &'static str,
) -> Result<Handle<super::Registry<CSS>>, HauchiwaError> {
    Ok(site_config.add_task_opaque(GlobRegistryTask::new(
        vec![glob_entry],
        vec![glob_watch],
        move |_, file| {
            let data = grass::from_path(&file.path, &grass::Options::default())?;
            let rt = Runtime;
            let path = rt.store(data.as_bytes(), "css")?;
            Ok((file.path, CSS { path }))
        },
    )?))
}
