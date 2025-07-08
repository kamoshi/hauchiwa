use std::any::{TypeId, type_name};
use std::ops::Deref;

use camino::Utf8Path;

use crate::{GitInfo, Globals, InputItem};
use crate::{Input, error::*};

const GLOB_OPTS: glob::MatchOptions = glob::MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: true,
};

#[derive(Debug)]
pub struct WithFile<'a, D> {
    pub file: &'a Utf8Path,
    pub slug: &'a Utf8Path,
    pub area: &'a Utf8Path,
    pub data: &'a D,
    pub info: Option<&'a GitInfo>,
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
    items: &'a Vec<&'a InputItem>,
}

impl<'a, G> Context<'a, G>
where
    G: Send + Sync,
{
    pub(crate) fn new(globals: &'a Globals<G>, items: &'a Vec<&'a InputItem>) -> Self {
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
                    && glob.matches_path_with(item.slug.as_std_path(), GLOB_OPTS)
            })
            .map(Deref::deref)
            .next()
            .ok_or_else(|| HauchiwaError::AssetNotFound(glob.to_string()))?;

        let data = match &item.data {
            Input::Lazy(lazy) => lazy.downcast_ref().unwrap(),
        };

        Ok(WithFile {
            file: &item.file,
            slug: &item.slug,
            area: &item.area,
            data,
            info: None,
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
                    && glob.matches_path_with(item.slug.as_std_path(), GLOB_OPTS)
            })
            .map(Deref::deref)
            .map(|item| {
                let data = match &item.data {
                    Input::Lazy(lazy) => lazy.downcast_ref().unwrap(),
                };

                WithFile {
                    file: &item.file,
                    slug: &item.slug,
                    area: &item.area,
                    data,
                    info: None,
                }
            })
            .collect();

        Ok(items)
    }

    /// Find the first on‐disk asset whose path matches `pattern`. This asset
    /// will be built only on request and cached.
    pub fn glob<T: 'static>(&self, pattern: &str) -> Result<Option<&T>, HauchiwaError> {
        let refl_type = TypeId::of::<T>();
        let glob = glob::Pattern::new(pattern)?;
        let next = self
            .items
            .iter()
            .filter(|item| {
                item.refl_type == refl_type
                    && glob.matches_path_with(item.file.as_std_path(), GLOB_OPTS)
            })
            .map(Deref::deref)
            .next();

        Ok(match next {
            Some(item) => match &item.data {
                Input::Lazy(lazy) => lazy.downcast_ref(),
            },
            None => None,
        })
    }

    pub fn get<T: 'static>(&self, path: &str) -> Result<&T, HauchiwaError> {
        let item = self.find_item_by_path(path)?;

        let data = match &item.data {
            Input::Lazy(lazy) => lazy.downcast_ref(),
        };

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

    fn find_item_by_path(&self, path: &str) -> Result<&InputItem, HauchiwaError> {
        self.items
            .iter()
            .map(Deref::deref)
            .find(|item| item.file == path)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))
    }
}
