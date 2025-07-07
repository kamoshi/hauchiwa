use std::{collections::HashSet, fs};

use camino::Utf8PathBuf;

use crate::{
    Hash32, InputItem,
    plugin::{Loadable, Runtime, content::LoaderContent, generic::LoaderGeneric},
};

pub struct Loader(Box<dyn Loadable>);

impl Loader {
    /// Create a new collection which draws content from the filesystem files
    /// via a glob pattern. Usually used to collect articles written as markdown
    /// files, however it is completely format agnostic.
    ///
    /// The parameter `parse_matter` allows you to customize how the metadata
    /// should be parsed. Default functions for the most common formats are
    /// provided by library:
    /// * [`parse_matter_json`](`crate::parse_matter_json`) - parse JSON metadata
    /// * [`parse_matter_yaml`](`crate::parse_matter_yaml`) - parse YAML metadata
    ///
    /// # Examples
    ///
    /// ```rust
    /// Collection::glob_with("content", "posts/**/*", ["md"], parse_matter_yaml::<Post>);
    /// ```
    pub fn glob_content<T>(
        path_base: &'static str,
        path_glob: &'static str,
        parse_matter: fn(&str) -> Result<(T, String), anyhow::Error>,
    ) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self(Box::new(LoaderContent::new(
            path_base,
            path_glob,
            parse_matter,
        )))
    }

    pub fn glob_asset<T>(
        path_base: &'static str,
        path_glob: &'static str,
        func: fn(Runtime, Vec<u8>) -> T,
    ) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::plugin(LoaderGeneric::new(
            path_base,
            path_glob,
            |path| {
                let data = fs::read(path).unwrap();
                let hash = Hash32::hash(&data);

                (hash, data)
            },
            func,
        ))
    }

    #[cfg(feature = "images")]
    pub fn glob_images(path_base: &'static str, path_glob: &'static str) -> Self {
        use crate::plugin::image::new_loader_image;

        Self::plugin(new_loader_image(path_base, path_glob))
    }

    pub fn glob_style(path_base: &'static str, path_glob: &'static str) -> Self {
        use crate::plugin::scss::new_loader_scss;

        Self::plugin(new_loader_scss(path_base, path_glob))
    }

    pub fn glob_scripts(path_base: &'static str, path_glob: &'static str) -> Self {
        use crate::plugin::ts::new_loader_ts;

        Self::plugin(new_loader_ts(path_base, path_glob))
    }

    pub fn glob_svelte(path_base: &'static str, path_glob: &'static str) -> Self {
        use crate::plugin::svelte::new_loader_svelte;

        Self::plugin(new_loader_svelte(path_base, path_glob))
    }

    fn plugin<T: Loadable>(plugin: T) -> Self {
        Self(Box::new(plugin))
    }

    pub(crate) fn load(&mut self) {
        self.0.load()
    }

    pub(crate) fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> bool {
        self.0.reload(set)
    }

    pub(crate) fn items(&self) -> Vec<&InputItem> {
        self.0.items()
    }

    pub(crate) fn path_base(&self) -> &'static str {
        self.0.path_base()
    }

    pub(crate) fn remove(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool {
        self.0.remove(obsolete)
    }
}
