use std::collections::{HashMap, HashSet};

use petgraph::graph::NodeIndex;

use crate::engine::{
    Dynamic, Provenance,
    task_f::{Map, Tracker, TrackerPtr},
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
        tracking: &Option<HashMap<String, Provenance>>,
        current: &Dynamic,
        updated: &HashSet<NodeIndex>,
    ) -> bool {
        // If the dependency has not been updated, it's valid.
        if !updated.contains(&self.index) {
            return true;
        }

        let tracking = match tracking {
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

        for (key, old_prov) in tracking {
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

        // All tracked items are unchanged
        true
    }
}
