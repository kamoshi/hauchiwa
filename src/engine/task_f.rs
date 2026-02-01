use std::{collections::HashMap, sync::Arc};

use glob::Pattern;
use petgraph::graph::NodeIndex;

use crate::error::HauchiwaError;

/// A collection of processed assets, indexed by their source file path.
///
/// `Assets<T>` is the standard return type for most loaders. It allows you to
/// access processed items (like posts or images) using their original file
/// path.
///
/// # Example
///
/// ```rust,no_run
/// # use hauchiwa::{Blueprint, task, loader::{Assets, Document}};
/// # #[derive(Clone, serde::Deserialize)]
/// # struct Post { title: String }
/// # let mut config = Blueprint::<()>::default();
/// # let posts = config.load_documents::<Post>().source("content/posts/*.md").register().unwrap();
/// # task!(config, |ctx, posts| {
/// // Assuming `posts` is a Assets<Document<Post>>
/// for post in posts.values() {
///     println!("Title: {}", post.matter.title);
/// }
///
/// let specific_post = posts.get("content/posts/hello.md")?;
/// # Ok(())
/// # });
/// ```
#[derive(Debug)]
pub(crate) struct Map<T> {
    pub(crate) map: HashMap<String, T>,
}

pub struct Tracker<'a, T> {
    pub(crate) map: &'a Map<T>,
}

impl<'a, T> Tracker<'a, T> {
    /// Retrieves a reference to the processed data for a given source path.
    pub fn get<K>(&self, key: K) -> Result<&T, HauchiwaError>
    where
        K: AsRef<str>,
    {
        self.map
            .map
            .get(key.as_ref())
            .ok_or(HauchiwaError::AssetNotFound(
                key.as_ref().to_string().into(),
            ))
    }

    /// Finds all items whose source paths match the given glob pattern.
    pub fn glob<P>(&self, pattern: P) -> Result<impl Iterator<Item = (&str, &T)>, HauchiwaError>
    where
        P: AsRef<str>,
    {
        let matcher = Pattern::new(pattern.as_ref())?;

        let iter = self.map.map.iter().filter_map(move |(key, val)| {
            let path = key.as_str();

            if matcher.matches(path) {
                Some((path, val))
            } else {
                None
            }
        });

        Ok(iter)
    }

    // This method borrows self immutable (&) and returns an iterator over &T
    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, String, T> {
        self.map.map.iter()
    }

    pub fn values(&self) -> std::collections::hash_map::Values<'_, String, T> {
        self.map.map.values()
    }
}

impl<'a, T> IntoIterator for Tracker<'a, T> {
    type Item = &'a T;
    type IntoIter = std::collections::hash_map::Values<'a, String, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.map.values()
    }
}

impl<'a, T> IntoIterator for &Tracker<'a, T> {
    type Item = &'a T;
    type IntoIter = std::collections::hash_map::Values<'a, String, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.map.values()
    }
}

pub(crate) trait TypedTaskF<G: Send + Sync = ()>: Send + Sync {
    /// The concrete output type of this task.
    type Output: Send + Sync + 'static;

    fn get_name(&self) -> String;

    fn dependencies(&self) -> Vec<NodeIndex>;

    fn get_watched(&self) -> Vec<camino::Utf8PathBuf>;

    fn execute(
        &self,
        context: &crate::TaskContext<G>,
        runtime: &mut crate::Store,
        dependencies: &[super::Dynamic],
    ) -> anyhow::Result<Map<Self::Output>>;

    fn is_dirty(&self, _: &camino::Utf8Path) -> bool {
        false
    }
}

/// The core trait for all tasks in the graph.
///
/// While most users will interact with the typed [`Blueprint::add_task`](crate::Blueprint::add_task)
/// API, this trait is the type-erased foundation that allows the graph to hold
/// tasks with different output types.
pub(crate) trait TaskF<G: Send + Sync = ()>: Send + Sync {
    fn get_name(&self) -> String;

    fn get_output_type_name(&self) -> &'static str;

    fn is_output(&self) -> bool;

    fn dependencies(&self) -> Vec<NodeIndex>;

    fn get_watched(&self) -> Vec<camino::Utf8PathBuf>;

    fn execute(
        &self,
        context: &crate::TaskContext<G>,
        runtime: &mut crate::Store,
        dependencies: &[super::Dynamic],
    ) -> anyhow::Result<super::Dynamic>;

    #[inline]
    fn is_dirty(&self, _: &camino::Utf8Path) -> bool {
        false
    }
}

// A blanket implementation to automatically bridge the two. This is where the
// type erasure actually happens.
impl<G, T> TaskF<G> for T
where
    G: Send + Sync,
    T: TypedTaskF<G> + 'static,
{
    fn get_name(&self) -> String {
        T::get_name(self)
    }

    fn get_output_type_name(&self) -> &'static str {
        std::any::type_name::<T::Output>()
    }

    fn is_output(&self) -> bool {
        use std::any::TypeId;

        TypeId::of::<T::Output>() == TypeId::of::<crate::Output>()
            || TypeId::of::<T::Output>() == TypeId::of::<Vec<crate::Output>>()
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        T::dependencies(self)
    }

    fn get_watched(&self) -> Vec<camino::Utf8PathBuf> {
        T::get_watched(self)
    }

    fn execute(
        &self,
        context: &crate::TaskContext<G>,
        runtime: &mut crate::Store,
        dependencies: &[super::Dynamic],
    ) -> anyhow::Result<super::Dynamic> {
        // Call the typed method, then erase the result.
        Ok(Arc::new(T::execute(self, context, runtime, dependencies)?))
    }

    fn is_dirty(&self, path: &camino::Utf8Path) -> bool {
        T::is_dirty(self, path)
    }
}
