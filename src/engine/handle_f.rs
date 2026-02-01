use std::collections::HashSet;

use glob::Pattern;
use petgraph::graph::NodeIndex;

use crate::engine::{
    Dynamic,
    task_f::{Map, Tracker, TrackerPtr, TrackerState},
};

pub struct HandleF<T> {
    pub(crate) index: NodeIndex,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> HandleF<T> {
    pub(crate) fn new(index: NodeIndex) -> Self {
        Self {
            index,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Returns the underlying `NodeIndex` of the task in the graph.
    pub fn index(&self) -> NodeIndex {
        self.index
    }
}

impl<T> Clone for HandleF<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for HandleF<T> {}

impl<T> super::Handle for HandleF<T>
where
    T: Send + Sync + 'static,
{
    type Output<'a> = Tracker<'a, T>;

    fn index(&self) -> NodeIndex {
        self.index
    }

    fn downcast<'a>(&self, output: &'a Dynamic) -> (Option<TrackerPtr>, Self::Output<'a>) {
        let ptr = TrackerPtr::default();

        let map = output
            .downcast_ref::<Map<T>>()
            .expect("Type mismatch in dependency resolution");

        (Some(ptr.clone()), Tracker { map, tracker: ptr })
    }

    fn is_valid(
        &self,
        tracking: &Option<TrackerState>,
        current: &Dynamic,
        updated: &HashSet<NodeIndex>,
    ) -> bool {
        // If the dependency has not been updated, it's valid.
        if !updated.contains(&self.index) {
            return true;
        }

        let state = match tracking {
            Some(tracking) => tracking,
            None => {
                // No tracking info but dependency changed -> assume invalid
                return false;
            }
        };

        // If we have previous tracking information
        let current = current
            .downcast_ref::<Map<T>>()
            .expect("Type mismatch in validation");

        // 1. Check specific accessed items (random access or iteration)
        for (key, old_prov) in &state.accessed {
            match current.map.get(key) {
                Some((_, new_prov)) => {
                    if old_prov != new_prov {
                        tracing::info!(
                            "Hash changed for key {} from {:?} to {:?}",
                            key,
                            old_prov,
                            new_prov,
                        );
                        return false;
                    }
                }
                None => {
                    tracing::info!("Key {} no longer exists", key);
                    return false;
                }
            }
        }

        // 2. Check full iteration consistency
        if state.iterated.count > 0 {
            let mut iter = current.map.iter();
            for _ in 0..state.iterated.count {
                match iter.next() {
                    Some((key, _)) => {
                        // The item must have been accessed previously.
                        // If we encounter an item that wasn't accessed, it means it's a new item
                        // inserted in the middle of our iteration range, effectively shifting/changing
                        // the sequence we saw.
                        if !state.accessed.contains_key(key) {
                            tracing::info!("Iteration encountered new item: {}", key);
                            return false;
                        }
                    }
                    None => {
                        // Iterator exhausted earlier than expected.
                        // We expected 'count' items, but found fewer.
                        tracing::info!("Iteration exhausted early");
                        return false;
                    }
                }
            }

            // If we previously exhausted the iterator, we must verify that there are no new items
            // appended to the end.
            if state.iterated.exhausted
                && let Some((key, _)) = iter.next()
            {
                tracing::info!("Iteration has new item at end: {}", key);
                return false;
            }
        }

        // 3. Check glob iteration consistency
        for (pattern, glob_state) in &state.globs {
            if glob_state.count > 0 || glob_state.exhausted {
                let matcher = match Pattern::new(pattern) {
                    Ok(p) => p,
                    Err(_) => {
                        // Pattern became invalid? Should not happen if it worked before.
                        return false;
                    }
                };

                // We simulate the glob iteration on the new map
                let mut iter = current.map.iter().filter(|(key, _)| matcher.matches(key));

                for _ in 0..glob_state.count {
                    match iter.next() {
                        Some((key, _)) => {
                            if !state.accessed.contains_key(key) {
                                tracing::info!("Glob {} encountered new item: {}", pattern, key);
                                return false;
                            }
                        }
                        None => {
                            tracing::info!("Glob {} exhausted early", pattern);
                            return false;
                        }
                    }
                }

                if glob_state.exhausted
                    && let Some((key, _)) = iter.next()
                {
                    tracing::info!("Glob {} has new item at end: {}", pattern, key);
                    return false;
                }
            }
        }

        // All checks passed
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Hash32;
    use crate::engine::{Handle, Provenance, task_f::IterationState};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn make_handle() -> HandleF<i32> {
        HandleF::new(NodeIndex::new(0))
    }

    fn make_map(items: Vec<(&str, i32)>) -> Dynamic {
        let mut map = BTreeMap::new();
        for (k, v) in items {
            map.insert(k.to_string(), (v, Provenance(Hash32::default())));
        }
        Arc::new(Map { map })
    }

    #[test]
    fn test_valid_access() {
        let handle = make_handle();
        let mut accessed = std::collections::HashMap::new();
        accessed.insert("a".to_string(), Provenance(Hash32::default()));

        let state = TrackerState {
            accessed,
            ..Default::default()
        };

        let current = make_map(vec![("a", 1), ("b", 2)]);
        let updated: HashSet<NodeIndex> = vec![NodeIndex::new(0)].into_iter().collect();

        assert!(handle.is_valid(&Some(state), &current, &updated));
    }

    #[test]
    fn test_invalid_access_missing() {
        let handle = make_handle();
        let mut accessed = std::collections::HashMap::new();
        accessed.insert("c".to_string(), Provenance(Hash32::default())); // 'c' missing

        let state = TrackerState {
            accessed,
            ..Default::default()
        };

        let current = make_map(vec![("a", 1), ("b", 2)]);
        let updated: HashSet<NodeIndex> = vec![NodeIndex::new(0)].into_iter().collect();

        assert!(!handle.is_valid(&Some(state), &current, &updated));
    }

    #[test]
    fn test_iter_valid() {
        let handle = make_handle();
        let mut accessed = std::collections::HashMap::new();
        accessed.insert("a".to_string(), Provenance(Hash32::default()));
        accessed.insert("b".to_string(), Provenance(Hash32::default()));

        let state = TrackerState {
            accessed,
            iterated: IterationState {
                count: 2,
                exhausted: false,
            },
            ..Default::default()
        };

        // Map has matching first 2 items
        let current = make_map(vec![("a", 1), ("b", 2), ("c", 3)]);
        let updated: HashSet<NodeIndex> = vec![NodeIndex::new(0)].into_iter().collect();

        assert!(handle.is_valid(&Some(state), &current, &updated));
    }

    #[test]
    fn test_iter_invalid_order() {
        let handle = make_handle();
        let mut accessed = std::collections::HashMap::new();
        accessed.insert("a".to_string(), Provenance(Hash32::default()));
        accessed.insert("b".to_string(), Provenance(Hash32::default()));

        let state = TrackerState {
            accessed,
            iterated: IterationState {
                count: 2,
                exhausted: false,
            },
            ..Default::default()
        };

        // "aa" inserted before "b", so "b" is pushed to index 2.
        // Iterator(2) sees "a", "aa". "aa" is not in accessed.
        let current = make_map(vec![("a", 1), ("aa", 2), ("b", 2)]);
        let updated: HashSet<NodeIndex> = vec![NodeIndex::new(0)].into_iter().collect();

        assert!(!handle.is_valid(&Some(state), &current, &updated));
    }

    #[test]
    fn test_iter_exhausted_check() {
        let handle = make_handle();
        let mut accessed = std::collections::HashMap::new();
        accessed.insert("a".to_string(), Provenance(Hash32::default()));

        let state = TrackerState {
            accessed,
            iterated: IterationState {
                count: 1,
                exhausted: true,
            },
            ..Default::default()
        };

        // Map has new item "b"
        let current = make_map(vec![("a", 1), ("b", 2)]);
        let updated: HashSet<NodeIndex> = vec![NodeIndex::new(0)].into_iter().collect();

        assert!(!handle.is_valid(&Some(state), &current, &updated));
    }
}
