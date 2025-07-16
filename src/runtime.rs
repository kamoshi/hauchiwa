use std::any::{TypeId, type_name};
use std::borrow::Cow;
use std::ops::Deref;
use std::sync::{Arc, RwLock};

use anyhow::anyhow;

use crate::{FileData, Hash32, error::*};
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
    items: &'a [&'a Item],
    ///
    deps: Arc<RwLock<Vec<Tracker>>>,
}

impl<'a, G> Context<'a, G>
where
    G: Send + Sync,
{
    pub(crate) fn new(
        globals: &'a Globals<G>,
        items: &'a [&'a Item],
        deps: Arc<RwLock<Vec<Tracker>>>,
    ) -> Self {
        Self {
            globals,
            items,
            deps,
        }
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
        let mut filter = FilterGlob::new(TypeId::of::<T>(), glob::Pattern::new(pattern)?);

        let item = filter
            .filter(self.items)
            .next()
            .ok_or_else(|| HauchiwaError::AssetNotFound(filter.glob.to_string()))?;

        let data = match &*item.data.data {
            Ok(ok) => ok,
            Err(e) => Err(e.clone())?,
        };

        // save dependencies
        filter.store(vec![item.hash]);
        self.deps.write().unwrap().push(Tracker::Glob(filter));

        Ok(WithFile {
            data: data.downcast_ref().ok_or(anyhow!("Failed to downcast"))?,
            file: item.data.file.clone(),
        })
    }

    pub fn glob_with_files<T>(&self, pattern: &str) -> Result<Vec<WithFile<'_, T>>, HauchiwaError>
    where
        T: 'static,
    {
        let mut filter = FilterGlob::new(TypeId::of::<T>(), glob::Pattern::new(pattern)?);

        let (items, hashes) = filter.filter(self.items).try_fold(
            (Vec::new(), Vec::new()),
            |mut acc, item| -> Result<_, HauchiwaError> {
                let data = match &*item.data.data {
                    Ok(ok) => ok,
                    Err(e) => Err(e.clone())?,
                };

                acc.0.push(WithFile {
                    data: data.downcast_ref().ok_or(anyhow!("Failed to downcast"))?,
                    file: item.data.file.clone(),
                });

                acc.1.push(item.hash);

                Ok(acc)
            },
        )?;

        // save dependencies
        filter.store(hashes);
        self.deps.write().unwrap().push(Tracker::Glob(filter));

        Ok(items)
    }

    /// Find the first on‐disk asset whose path matches `pattern`. This asset
    /// will be built only on request and cached.
    pub fn glob<T: 'static>(&self, pattern: &str) -> Result<Option<&T>, HauchiwaError> {
        let mut filter = FilterGlob::new(TypeId::of::<T>(), glob::Pattern::new(pattern)?);
        let item = filter.filter(self.items).next();

        let item = match item {
            Some(item) => item,
            None => return Ok(None),
        };

        let data = match &*item.data.data {
            Ok(ok) => ok,
            Err(e) => Err(e.clone())?,
        };

        // save dependencies
        filter.store(vec![item.hash]);
        self.deps.write().unwrap().push(Tracker::Glob(filter));

        Ok(data.downcast_ref())
    }

    pub fn get<T: 'static>(&self, path: &str) -> Result<&T, HauchiwaError> {
        let mut filter = FilterId::new(path);
        let item = filter
            .filter(self.items)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))?;

        let data = match &*item.data.data {
            Ok(ok) => match ok.downcast_ref() {
                Some(data) => data,
                None => {
                    let have = item.refl_name;
                    let need = type_name::<T>();
                    Err(HauchiwaError::Frontmatter(format!(
                        "Requested {need}, but received {have}"
                    )))?
                }
            },
            Err(e) => Err(e.clone())?,
        };

        // save dependencies
        filter.store(item);
        self.deps.write().unwrap().push(Tracker::Id(filter));

        Ok(data)
    }
}

struct FilterId {
    id: String,
    hash: Hash32,
}

impl FilterId {
    fn new(id: impl ToString) -> Self {
        Self {
            id: id.to_string(),
            hash: Default::default(),
        }
    }

    fn filter<'ctx>(&self, items: &'ctx [&Item]) -> Option<&'ctx Item> {
        items
            .iter()
            .map(Deref::deref)
            .find(|item| item.data.file.file == self.id)
    }

    fn check(&self, items: &[&Item]) -> bool {
        match self.filter(items) {
            Some(item) => self.hash != item.hash,
            None => true,
        }
    }

    fn store(&mut self, item: &Item) {
        self.hash = item.hash;
    }
}

struct FilterGlob {
    ty: TypeId,
    glob: glob::Pattern,
    hash: Cow<'static, [Hash32]>,
}

impl FilterGlob {
    fn new(ty: TypeId, glob: glob::Pattern) -> Self {
        Self {
            ty,
            glob,
            hash: Default::default(),
        }
    }

    fn filter<'ctx>(&self, items: &'ctx [&'ctx Item]) -> impl Iterator<Item = &'ctx Item> {
        items
            .iter()
            .filter(|item| {
                item.refl_type == self.ty
                    && self
                        .glob
                        .matches_path_with(item.data.file.slug.as_std_path(), GLOB_OPTS)
            })
            .map(Deref::deref)
    }

    fn check(&self, items: &[&Item]) -> bool {
        let new = self.filter(items).collect::<Vec<_>>();
        if self.hash.len() != new.len() {
            return true;
        }

        for item in new {
            if !self.hash.contains(&item.hash) {
                return true;
            }
        }

        false
    }

    fn store(&mut self, items: Vec<Hash32>) {
        self.hash = if items.is_empty() {
            Default::default()
        } else {
            items.into()
        };
    }
}

pub enum Tracker {
    Id(FilterId),
    Glob(FilterGlob),
}

impl Tracker {
    pub fn check(&self, items: &[&Item]) -> bool {
        match self {
            Tracker::Id(filter) => filter.check(items),
            Tracker::Glob(filter) => filter.check(items),
        }
    }
}
