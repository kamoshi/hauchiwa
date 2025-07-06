use std::collections::HashSet;

use camino::Utf8PathBuf;

use crate::InputItem;

pub mod generic;
#[cfg(feature = "images")]
pub mod image;

pub(crate) trait Loadable: 'static + Send {
    fn load(&mut self);
    fn reload(&mut self, set: &HashSet<Utf8PathBuf>) -> bool;
    fn items(&self) -> Vec<&InputItem>;
    fn path_base(&self) -> &'static str;
    fn remove(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool;
}
