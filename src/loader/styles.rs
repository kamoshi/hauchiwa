use crate::{
    SiteConfig,
    loader::{Runtime, glob::GlobLoaderTask},
    task::Handle,
};
use camino::Utf8PathBuf;

#[derive(Clone)]
pub struct CSS {
    pub path: camino::Utf8PathBuf,
}

pub fn build_styles<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    glob_entry: &'static str,
    glob_watch: &'static str,
) -> Handle<super::Registry<CSS>> {
    let handle_styles = site_config.add_task_opaque(GlobLoaderTask::new(
        glob_entry,
        glob_watch,
        move |_, file| {
            let data = grass::from_path(&file.path, &grass::Options::default())?;
            let rt = Runtime;
            let path = rt.store(data.as_bytes(), "css")?;
            Ok((file.path, CSS { path }))
        },
    ));

    site_config.add_task(
        (handle_styles,),
        |_, (styles_vec,): (&Vec<(Utf8PathBuf, CSS)>,)| super::Registry {
            map: styles_vec.iter().cloned().collect(),
        },
    )
}
