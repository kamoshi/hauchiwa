use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, Mutex},
};

use glob::Pattern;
use petgraph::graph::NodeIndex;

use crate::engine::Dynamic;
use crate::{Hash32, error::HauchiwaError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Provenance(pub(crate) Hash32);

/// A collection of processed assets, indexed by their source file path.
///
/// `Assets<T>` is the standard return type for most loaders. It allows you to
/// access processed items (like posts or images) using their original file
/// path.
#[derive(Debug)]
pub(crate) struct Map<T> {
    pub(crate) map: BTreeMap<String, (T, Provenance)>,
}

pub struct Tracker<'a, T> {
    pub(crate) map: &'a Map<T>,
    pub(crate) tracker: TrackerPtr,
}

#[derive(Clone, Default, Debug)]
pub struct IterationState {
    pub count: usize,
    pub exhausted: bool,
}

#[derive(Clone, Default, Debug)]
pub struct TrackerState {
    pub accessed: HashMap<String, Provenance>,
    pub globs: HashMap<String, IterationState>,
    pub iterated: IterationState,
}

#[derive(Clone, Default)]
pub struct TrackerPtr {
    pub(crate) ptr: Arc<Mutex<TrackerState>>,
}

impl<'a, T> Tracker<'a, T> {
    /// Retrieves a reference to the processed data for a given source path.
    pub fn get<K>(&self, key: K) -> Result<&T, HauchiwaError>
    where
        K: AsRef<str>,
    {
        match self.map.map.get(key.as_ref()) {
            Some((item, provenance)) => {
                let mut tracker = self.tracker.ptr.lock().unwrap();
                tracker
                    .accessed
                    .insert(key.as_ref().to_string(), *provenance);

                Ok(item)
            }
            None => Err(HauchiwaError::AssetNotFound(
                key.as_ref().to_string().into(),
            )),
        }
    }

    /// Finds all items whose source paths match the given glob pattern.
    pub fn glob<P>(&self, pattern: P) -> Result<impl Iterator<Item = (&str, &T)>, HauchiwaError>
    where
        P: AsRef<str>,
    {
        let pattern_str = pattern.as_ref().to_string();
        let matcher = Pattern::new(&pattern_str)?;
        let tracker = self.tracker.ptr.clone();

        // We box the iterator to simplify the type signature
        let iter = Box::new(
            self.map
                .map
                .iter()
                .filter(move |(key, _)| matcher.matches(key)),
        );

        Ok(TrackerGlobIter {
            iter,
            tracker,
            pattern: pattern_str,
            count: 0,
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &T)> {
        TrackerIter {
            iter: self.map.map.iter(),
            tracker: self.tracker.ptr.clone(),
            count: 0,
        }
    }

    pub fn values(&self) -> Box<dyn Iterator<Item = &T> + '_> {
        Box::new(
            TrackerIter {
                iter: self.map.map.iter(),
                tracker: self.tracker.ptr.clone(),
                count: 0,
            }
            .map(|(_, item)| item),
        )
    }
}

pub struct TrackerIter<'a, T> {
    iter: std::collections::btree_map::Iter<'a, String, (T, Provenance)>,
    tracker: Arc<Mutex<TrackerState>>,
    count: usize,
}

impl<'a, T> Iterator for TrackerIter<'a, T> {
    type Item = (&'a String, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.iter.next();
        let mut tracker = self.tracker.lock().unwrap();

        match next {
            Some((key, (item, provenance))) => {
                self.count += 1;
                tracker.iterated.count = tracker.iterated.count.max(self.count);
                tracker.accessed.insert(key.clone(), *provenance);
                Some((key, item))
            }
            None => {
                tracker.iterated.exhausted = true;
                None
            }
        }
    }
}

pub struct TrackerGlobIter<'a, T> {
    iter: Box<dyn Iterator<Item = (&'a String, &'a (T, Provenance))> + 'a>,
    tracker: Arc<Mutex<TrackerState>>,
    pattern: String,
    count: usize,
}

impl<'a, T> Iterator for TrackerGlobIter<'a, T> {
    type Item = (&'a str, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.iter.next();
        let mut tracker = self.tracker.lock().unwrap();

        match next {
            Some((key, (item, provenance))) => {
                self.count += 1;
                let state = tracker.globs.entry(self.pattern.clone()).or_default();
                state.count = state.count.max(self.count);
                tracker.accessed.insert(key.clone(), *provenance);
                Some((key.as_str(), item))
            }
            None => {
                let state = tracker.globs.entry(self.pattern.clone()).or_default();
                state.exhausted = true;
                None
            }
        }
    }
}

impl<'a, T> IntoIterator for Tracker<'a, T> {
    type Item = &'a T;
    type IntoIter = Box<dyn Iterator<Item = &'a T> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(
            TrackerIter {
                iter: self.map.map.iter(),
                tracker: self.tracker.ptr.clone(),
                count: 0,
            }
            .map(|(_, item)| item),
        )
    }
}

impl<'a, 'b, T> IntoIterator for &'b Tracker<'a, T> {
    type Item = &'a T;
    type IntoIter = Box<dyn Iterator<Item = &'a T> + 'b>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(
            TrackerIter {
                iter: self.map.map.iter(),
                tracker: self.tracker.ptr.clone(),
                count: 0,
            }
            .map(|(_, item)| item),
        )
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

    fn is_valid(&self, _: &[Option<TrackerState>], _: &[Dynamic], _: &HashSet<NodeIndex>) -> bool {
        true
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

    fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool;
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

    fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool {
        T::is_valid(self, old_tracking, new_outputs, updated_nodes)
    }
}
