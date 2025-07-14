use std::any::{TypeId, type_name};
use std::ops::Deref;
use std::sync::Arc;

use anyhow::anyhow;

use crate::{FileData, error::*};
use crate::{Globals, Item};

const GLOB_OPTS: glob::MatchOptions = glob::MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: true,
};

pub struct WithFile<'a, D> {
    pub data: &'a D,
    pub file: Arc<FileData>,
}

/// A simple wrapper for all context data passed at runtime to tasks defined for
/// the website. Use this struct's methods to query required data.
pub struct Context<'a, G>
where
    G: Send + Sync,
{
    /// Global data for the current build.
    globals: &'a Globals<G>,
    /// Every single input.
    items: &'a Vec<&'a Item>,
}

impl<'a, G> Context<'a, G>
where
    G: Send + Sync,
{
    pub(crate) fn new(globals: &'a Globals<G>, items: &'a Vec<&'a Item>) -> Self {
        Self { globals, items }
    }

    /// Retrieve the globals.
    pub fn get_globals(&self) -> &Globals<G> {
        self.globals
    }

    /// Get the JS script which enables live reloading.
    pub fn get_refresh_script(&self) -> Option<String> {
        self.globals.port.map(|port| {
            format!(
                r#"
const socket = new WebSocket("ws://localhost:{port}");
socket.addEventListener("message", event => {{
    window.location.reload();
}});
"#
            )
        })
    }

    pub fn glob_with_file<T>(&self, pattern: &str) -> Result<WithFile<'_, T>, HauchiwaError>
    where
        T: 'static,
    {
        let refl_type = TypeId::of::<T>();
        let glob = glob::Pattern::new(pattern)?;

        let item = self
            .items
            .iter()
            .filter(|item| {
                item.refl_type == refl_type
                    && glob.matches_path_with(item.data.file.slug.as_std_path(), GLOB_OPTS)
            })
            .map(Deref::deref)
            .next()
            .ok_or_else(|| HauchiwaError::AssetNotFound(glob.to_string()))?;

        let data = match &*item.data.data {
            Ok(ok) => ok,
            Err(e) => Err(e.clone())?,
        };

        Ok(WithFile {
            data: data.downcast_ref().ok_or(anyhow!("Failed to downcast"))?,
            file: item.data.file.clone(),
        })
    }

    pub fn glob_with_files<T>(&self, pattern: &str) -> Result<Vec<WithFile<'_, T>>, HauchiwaError>
    where
        T: 'static,
    {
        let refl_type = TypeId::of::<T>();
        let glob = glob::Pattern::new(pattern)?;

        let items = self
            .items
            .iter()
            .filter(|item| {
                item.refl_type == refl_type
                    && glob.matches_path_with(item.data.file.slug.as_std_path(), GLOB_OPTS)
            })
            .try_fold(Vec::new(), |mut acc, &item| -> Result<_, HauchiwaError> {
                let data = match &*item.data.data {
                    Ok(ok) => ok,
                    Err(e) => Err(e.clone())?,
                };

                acc.push(WithFile {
                    data: data.downcast_ref().ok_or(anyhow!("Failed to downcast"))?,
                    file: item.data.file.clone(),
                });

                Ok(acc)
            })?;

        Ok(items)
    }

    /// Find the first on‐disk asset whose path matches `pattern`. This asset
    /// will be built only on request and cached.
    pub fn glob<T: 'static>(&self, pattern: &str) -> Result<Option<&T>, HauchiwaError> {
        let refl_type = TypeId::of::<T>();
        let glob = glob::Pattern::new(pattern)?;
        let item = self
            .items
            .iter()
            .filter(|item| {
                item.refl_type == refl_type
                    && glob.matches_path_with(item.data.file.file.as_std_path(), GLOB_OPTS)
            })
            .map(Deref::deref)
            .next();

        let item = match item {
            Some(item) => item,
            None => return Ok(None),
        };

        let data = match &*item.data.data {
            Ok(ok) => ok,
            Err(e) => Err(e.clone())?,
        };

        Ok(data.downcast_ref())
    }

    pub fn get<T: 'static>(&self, path: &str) -> Result<&T, HauchiwaError> {
        let item = self.find_item_by_path(path)?;
        let data = match &*item.data.data {
            Ok(ok) => ok,
            Err(e) => Err(e.clone())?,
        };
        let data = data.downcast_ref();

        match data {
            Some(data) => Ok(data),
            None => {
                let have = item.refl_name;
                let need = type_name::<T>();
                eprintln!("Requested {need}, but received {have}");
                todo!()
            }
        }
    }

    fn find_item_by_path(&self, path: &str) -> Result<&Item, HauchiwaError> {
        self.items
            .iter()
            .map(Deref::deref)
            .find(|item| item.data.file.file == path)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))
    }
}
