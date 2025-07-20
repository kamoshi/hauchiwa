use std::any::TypeId;
use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Deref;
use std::sync::{Arc, RwLock};

use crate::{FileData, Hash32, error::*};
use crate::{Globals, Item};

const GLOB_OPTS: glob::MatchOptions = glob::MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: true,
};

/// Associates runtime artifacts (e.g., parsed content, structured data or
/// images) with its originating file metadata.
pub struct WithFile<'a, D> {
    pub data: &'a D,
    pub file: Arc<FileData>,
}

/// Runtime container for all assets available to build tasks.
///
/// Encapsulates per-invocation build state, including the global configuration,
/// all registered input items (typed assets, lazily evaluated), and a dependency
/// tracker to record reads for incremental builds. This struct mediates safe,
/// type-driven access to the task's data environment.
///
/// Use `get`, `glob`, and related methods to retrieve assets by identifier
/// or glob pattern, while automatically tracking usage for fine-grained invalidation.
pub struct Context<'a, G>
where
    G: Send + Sync,
{
    /// Global data for the current build.
    globals: &'a Globals<G>,
    /// Every single input.
    items: &'a [&'a Item],
    deps: Arc<RwLock<Vec<Tracker>>>,
}

impl<'a, G> Context<'a, G>
where
    G: Send + Sync,
{
    /// Constructs a new task-scoped context from global state and item registry.
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

    /// Returns a shared reference to the current build's global configuration.
    ///
    /// Useful for accessing user-specified parameters, environment info, or
    /// other resources passed in at runtime.
    pub fn get_globals(&self) -> &Globals<G> {
        self.globals
    }

    /// If live reload is enabled, returns an inline JavaScript snippet to
    /// establish a WebSocket connection for hot page refresh during
    /// development.
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

    /// Retrieves a single item of the specified type `T` by identifier.
    ///
    /// The requested type must match exactly the shape under which the item was registered.
    /// If the identifier exists but the type is wrong, an error variant will indicate the
    /// available types.
    pub fn get<T: 'static>(&self, id: impl AsRef<str>) -> Result<&T, ContextError> {
        let mut filter = FilterId::new(TypeId::of::<T>(), id.as_ref());
        let item = match filter.filter(self.items) {
            Some(item) => item,
            None => {
                let other = filter.other_types(self.items);
                let id = id.as_ref().to_string();
                return Err(match other.len() {
                    0 => ContextError::NotFound(id),
                    _ => ContextError::NotFoundWrongShape(id, other.join(", ")),
                });
            }
        };

        let data = match &*item.data.data {
            Ok(ok) => ok.downcast_ref().unwrap(), // this won't ever fail
            Err(e) => return Err(ContextError::LazyAssetError(item.id.to_string(), e.clone())),
        };

        // save dependencies
        filter.store(item);
        self.deps.write().unwrap().push(Tracker::Id(filter));

        Ok(data)
    }

    /// Finds the first matching item of type `T` using a glob pattern.
    ///
    /// Returns an error if no match is found, or if the match is of the wrong type.
    /// Only the first match is returned; use `glob()` for multi-match patterns.
    pub fn glob_one<T: 'static>(&self, pattern: &str) -> Result<&T, ContextError> {
        let mut filter = FilterGlob::new(TypeId::of::<T>(), glob::Pattern::new(pattern)?);

        let item = match filter.filter(self.items).next() {
            Some(item) => item,
            None => {
                let other = filter.other_types(self.items).join(", ");
                return Err(ContextError::NotFoundWrongShape(pattern.to_string(), other));
            }
        };

        let data = match &*item.data.data {
            Ok(ok) => ok,
            Err(e) => return Err(ContextError::LazyAssetError(item.id.to_string(), e.clone())),
        };

        // save dependencies
        filter.store(vec![item.hash]);
        self.deps.write().unwrap().push(Tracker::Glob(filter));

        Ok(data.downcast_ref().unwrap())
    }

    /// Retrieves all items of type `T` matching the given glob pattern.
    ///
    /// Items must match both the pattern and the expected type.
    pub fn glob<T: 'static>(&self, pattern: &str) -> Result<Vec<&T>, ContextError> {
        let mut filter = FilterGlob::new(TypeId::of::<T>(), glob::Pattern::new(pattern)?);
        let (data, hash) =
            filter
                .filter(self.items)
                .try_fold((Vec::new(), Vec::new()), |mut acc, item| {
                    let data = match &*item.data.data {
                        Ok(ok) => ok,
                        Err(e) => {
                            return Err(ContextError::LazyAssetError(
                                item.id.to_string(),
                                e.clone(),
                            ));
                        }
                    };

                    acc.0.push(data.downcast_ref().unwrap());
                    acc.1.push(item.hash);

                    Ok(acc)
                })?;

        // save dependencies
        filter.store(hash);
        self.deps.write().unwrap().push(Tracker::Glob(filter));

        Ok(data)
    }

    /// Like `glob_one`, but returns the matching item paired with its file metadata.
    pub fn glob_file<T: 'static>(&self, pattern: &str) -> Result<WithFile<'_, T>, ContextError> {
        let mut filter = FilterGlob::new(TypeId::of::<T>(), glob::Pattern::new(pattern)?);

        let item = match filter.filter(self.items).next() {
            Some(item) => item,
            None => {
                let other = filter.other_types(self.items).join(", ");
                return Err(ContextError::NotFoundWrongShape(pattern.to_string(), other));
            }
        };

        let data = match &*item.data.data {
            Ok(ok) => ok,
            Err(e) => return Err(ContextError::LazyAssetError(item.id.to_string(), e.clone())),
        };

        // save dependencies
        filter.store(vec![item.hash]);
        self.deps.write().unwrap().push(Tracker::Glob(filter));

        Ok(WithFile {
            data: data.downcast_ref().unwrap(),
            file: item.data.file.clone(),
        })
    }

    /// Like `glob`, but returns all matching items along with their file metadata.
    pub fn glob_files<T>(&self, pattern: &str) -> Result<Vec<WithFile<'_, T>>, ContextError>
    where
        T: 'static,
    {
        let mut filter = FilterGlob::new(TypeId::of::<T>(), glob::Pattern::new(pattern)?);

        let (items, hashes) = filter.filter(self.items).try_fold(
            (Vec::new(), Vec::new()),
            |mut acc, item| -> Result<_, ContextError> {
                let data = match &*item.data.data {
                    Ok(ok) => ok,
                    Err(e) => {
                        return Err(ContextError::LazyAssetError(item.id.to_string(), e.clone()));
                    }
                };

                let data = data.downcast_ref().unwrap();
                let file = item.data.file.clone();

                acc.0.push(WithFile { data, file });
                acc.1.push(item.hash);

                Ok(acc)
            },
        )?;

        // save dependencies
        filter.store(hashes);
        self.deps.write().unwrap().push(Tracker::Glob(filter));

        Ok(items)
    }
}

pub(crate) struct FilterId {
    ty: TypeId,
    id: String,
    hash: Hash32,
}

impl FilterId {
    fn new(ty: TypeId, id: &str) -> Self {
        Self {
            ty,
            id: id.to_string(),
            hash: Default::default(),
        }
    }

    fn filter<'ctx>(&self, items: &'ctx [&Item]) -> Option<&'ctx Item> {
        items
            .iter()
            .find(|item| item.refl_type == self.ty && *item.id == self.id)
            .map(Deref::deref)
    }

    fn other_types(&self, items: &[&Item]) -> Vec<&'static str> {
        items
            .iter()
            .filter_map(|item| {
                if item.refl_type != self.ty && *item.id == self.id {
                    Some(item.refl_name)
                } else {
                    None
                }
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
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

pub(crate) struct FilterGlob {
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

    fn other_types<'ctx>(&self, items: &'ctx [&'ctx Item]) -> Vec<&'static str> {
        items
            .iter()
            .filter_map(|item| {
                if item.refl_type != self.ty
                    && self
                        .glob
                        .matches_path_with(item.data.file.slug.as_std_path(), GLOB_OPTS)
                {
                    Some(item.refl_name)
                } else {
                    None
                }
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
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
