use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glob::Pattern;

use crate::core::ArcStr;
use crate::engine::{Map, Provenance};
use crate::error::HauchiwaError;

#[derive(Clone, Default, Debug)]
pub struct IterationState {
    pub count: usize,
    pub exhausted: bool,
}

#[derive(Clone, Default, Debug)]
pub struct TrackerState {
    pub accessed: HashMap<ArcStr, Provenance>,
    pub globs: HashMap<String, IterationState>,
    pub iterated: IterationState,
}

#[derive(Clone, Default)]
pub struct TrackerPtr {
    pub(crate) ptr: Arc<Mutex<TrackerState>>,
}

#[derive(Default)]
pub struct Tracking {
    pub edges: Vec<Option<TrackerPtr>>,
}

impl Tracking {
    pub(crate) fn unwrap(self) -> Vec<Option<TrackerState>> {
        self.edges
            .into_iter()
            .map(|edge| edge.map(|item| Arc::try_unwrap(item.ptr).unwrap().into_inner().unwrap()))
            .collect()
    }
}

/// A collection of assets tracked with fine-grained granularity.
///
/// `Tracker` is the primary way to access the output of tasks that produce
/// multiple items.
///
/// # Granularity
///
/// When you access items through `Tracker` (e.g., via [`get`](Self::get) or
/// [`glob`](Self::glob)), the build system records exactly which items your
/// task depends on.
///
/// * If you read file "A", and file "B" changes, your task will **not** re-run.
/// * If you iterate over all files, your task **will** re-run if any file
///   changes or is added/removed.
pub struct Tracker<'a, T> {
    pub(crate) map: &'a Map<T>,
    pub(crate) tracker: TrackerPtr,
}

impl<'a, T> Tracker<'a, T> {
    /// Retrieves a reference to the processed data for a given source path.
    pub fn get<K>(&self, key: K) -> Result<&T, HauchiwaError>
    where
        K: AsRef<str>,
    {
        let key = key.as_ref();

        match self.map.map.get_key_value(key) {
            Some((key, (item, provenance))) => {
                let mut inner = self.tracker.ptr.lock().unwrap();
                inner.accessed.insert(key.clone(), *provenance);

                Ok(item)
            }
            None => Err(HauchiwaError::AssetNotFound(key.into())),
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
        let iter = Box::new(self.map.map.iter().filter_map(move |(key, val)| {
            if matcher.matches(key) {
                Some((key, val))
            } else {
                None
            }
        }));

        Ok(TrackerGlobIter {
            iter,
            tracker,
            pattern: pattern_str,
            count: 0,
        })
    }

    /// Iterates over all tracked items (key and value).
    ///
    /// **Warning:** Iterating over the entire collection marks your task as dependent
    /// on the *entire set*. This means your task will re-run if *any* item is added,
    /// removed, or modified.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        TrackerIter {
            iter: self.map.map.iter(),
            tracker: self.tracker.ptr.clone(),
            count: 0,
        }
    }

    /// Iterates over all tracked values.
    ///
    /// **Warning:** Like [`iter`](Self::iter), this creates a dependency on the entire collection.
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
    iter: std::collections::btree_map::Iter<'a, Arc<str>, (T, Provenance)>,
    tracker: Arc<Mutex<TrackerState>>,
    count: usize,
}

impl<'a, T> Iterator for TrackerIter<'a, T> {
    type Item = (&'a str, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.iter.next();
        let mut tracker = self.tracker.lock().unwrap();

        match next {
            Some((key, (item, provenance))) => {
                self.count += 1;
                tracker.iterated.count = tracker.iterated.count.max(self.count);
                tracker.accessed.insert(key.clone(), *provenance);
                Some((key.as_ref(), item))
            }
            None => {
                tracker.iterated.exhausted = true;
                None
            }
        }
    }
}

pub struct TrackerGlobIter<'a, T> {
    iter: Box<dyn Iterator<Item = (&'a ArcStr, &'a (T, Provenance))> + 'a>,
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
                tracker.accessed.insert(Arc::clone(key), *provenance);
                Some((key.as_ref(), item))
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
