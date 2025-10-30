use camino::Utf8PathBuf;
use grass::{Options, OutputStyle};

use crate::{
    loader::{File, FileLoaderTask, Runtime},
    task::Handle,
    SiteConfig,
};

/// Represents a compiled CSS asset emitted by the build pipeline.
///
/// This struct contains only the path to the minified stylesheet,
/// which can be included in HTML templates or referenced from other assets.
#[derive(Clone)]
pub struct Style {
    /// Path to the generated CSS file.
    pub path: Utf8PathBuf,
}

pub fn glob_styles<G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path_base: &'static str,
    path_glob: &'static str,
) -> Handle<Vec<Style>> {
    let task = FileLoaderTask::new(path_base, path_glob, move |_globals, file| {
        let opts = Options::default().style(OutputStyle::Compressed);
        let data = grass::from_string(String::from_utf8(file.metadata)?, &opts)?;
        let rt = Runtime;
        let path = rt.store(data.as_bytes(), "css")?;
        Ok(Style { path })
    });
    site_config.add_task_boxed(Box::new(task))
}
