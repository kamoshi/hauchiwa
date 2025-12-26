use crate::{SiteConfig, error::HauchiwaError, loader::glob::GlobRegistryTask, task::Handle};

#[derive(Debug, Clone)]
pub struct CSS {
    pub path: camino::Utf8PathBuf,
}

impl<G> SiteConfig<G>
where
    G: Send + Sync + 'static,
{
    pub fn build_styles(
        &mut self,
        glob_entry: &'static str,
        glob_watch: &'static str,
    ) -> Result<Handle<super::Registry<CSS>>, HauchiwaError> {
        Ok(self.add_task_opaque(GlobRegistryTask::new(
            vec![glob_entry],
            vec![glob_watch],
            move |_, rt, file| {
                let data = grass::from_path(&file.path, &grass::Options::default())?;
                let path = rt.store(data.as_bytes(), "css")?;

                Ok((file.path, CSS { path }))
            },
        )?))
    }
}
