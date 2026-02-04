use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::hash::Hash;
use std::marker::PhantomData;

use petgraph::graph::NodeIndex;

use crate::Many;
use crate::core::{Blake3Hasher, Dynamic, Store, TaskContext};
use crate::engine::{
    Dependencies, Map, Provenance, TrackerState, Tracking, TypedCoarse, TypedFine,
};

/// Squash dependencies into one output
/// Dependencies -> One<R>
pub(crate) struct NodeGather<G, R, D, F>
where
    G: Send + Sync,
    R: Send + Sync + 'static,
    D: Dependencies,
    F: for<'a> Fn(&TaskContext<'a, G>, D::Output<'a>) -> anyhow::Result<R> + Send + Sync,
{
    pub name: Cow<'static, str>,
    pub dependencies: D,
    pub callback: F,
    pub _phantom: PhantomData<G>,
}

impl<G, R, D, F> TypedCoarse<G> for NodeGather<G, R, D, F>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
    D: Dependencies + Send + Sync,
    F: for<'a> Fn(&TaskContext<'a, G>, D::Output<'a>) -> anyhow::Result<R> + Send + Sync + 'static,
{
    type Output = R;

    fn get_name(&self) -> String {
        self.name.to_string()
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        self.dependencies.dependencies()
    }

    fn get_watched(&self) -> Vec<camino::Utf8PathBuf> {
        vec![]
    }

    fn execute(
        &self,
        context: &TaskContext<G>,
        _: &mut Store,
        dependencies: &[Dynamic],
    ) -> anyhow::Result<(Tracking, Self::Output)> {
        let (tracking, dependencies) = self.dependencies.resolve(dependencies);
        let output = (self.callback)(context, dependencies)?;
        Ok((tracking, output))
    }

    fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool {
        self.dependencies
            .is_valid(old_tracking, new_outputs, updated_nodes)
    }
}

/// Explode dependencies into multiple outputs
/// Dependencies -> Many<R>
///
/// Constraints:
/// - R must be Hash, because it will be used for tracking
pub(crate) struct NodeScatter<G, R, D, F>
where
    G: Send + Sync,
    R: Send + Sync + std::hash::Hash + 'static,
    D: Dependencies,
    F: for<'a> Fn(&TaskContext<'a, G>, D::Output<'a>) -> anyhow::Result<Vec<(String, R)>>
        + Send
        + Sync,
{
    pub name: Cow<'static, str>,
    pub dependencies: D,
    pub callback: F,
    pub _phantom: PhantomData<G>,
}

impl<G, R, D, F> TypedFine<G> for NodeScatter<G, R, D, F>
where
    G: Send + Sync + 'static,
    R: Send + Sync + Hash + 'static,
    D: Dependencies + Send + Sync,
    F: for<'a> Fn(&TaskContext<'a, G>, D::Output<'a>) -> anyhow::Result<Vec<(String, R)>>
        + Send
        + Sync
        + 'static,
{
    type Output = R;

    fn get_name(&self) -> String {
        self.name.to_string()
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        self.dependencies.dependencies()
    }

    fn get_watched(&self) -> Vec<camino::Utf8PathBuf> {
        vec![]
    }

    fn execute(
        &self,
        context: &TaskContext<G>,
        _: &mut Store,
        dependencies: &[Dynamic],
        _old_output: Option<&Dynamic>,
        _updated_nodes: &HashSet<NodeIndex>,
    ) -> anyhow::Result<Map<Self::Output>> {
        let (_, inputs) = self.dependencies.resolve(dependencies);

        let items = (self.callback)(context, inputs)?;

        let mut map = std::collections::BTreeMap::new();

        for (key, item) in items {
            let hash = {
                let mut hasher = Blake3Hasher::default();
                item.hash(&mut hasher);
                hasher.into()
            };

            let provenance = Provenance(hash);

            map.insert(key.into(), (item, provenance));
        }

        Ok(Map { map, dirty: false })
    }

    fn is_valid(
        &self,
        _old_tracking: &[Option<TrackerState>],
        _new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool {
        for dep in self.dependencies.dependencies() {
            if updated_nodes.contains(&dep) {
                return false;
            }
        }
        true
    }
}

/// Map each input to a single output, with additional (side) dependencies
/// Many<T> -> Many<R>
pub(crate) struct NodeMap<T, G, R, D, F>
where
    T: Send + Sync + 'static,
    G: Send + Sync + 'static,
    R: Send + Sync + Clone + 'static,
    D: Dependencies,
    F: for<'a> Fn(&TaskContext<'a, G>, &T, D::Output<'a>) -> anyhow::Result<R> + Send + Sync,
{
    pub name: Cow<'static, str>,
    pub dep_primary: Many<T>,
    pub dep_secondary: D,
    pub callback: F,
    pub _phantom: PhantomData<G>,
}

impl<T, G, R, D, F> TypedFine<G> for NodeMap<T, G, R, D, F>
where
    T: Send + Sync + 'static,
    G: Send + Sync + 'static,
    R: Send + Sync + Clone + 'static,
    D: Dependencies + Send + Sync,
    F: for<'a> Fn(&TaskContext<'a, G>, &T, D::Output<'a>) -> anyhow::Result<R> + Send + Sync,
{
    type Output = R;

    fn get_name(&self) -> String {
        self.name.to_string()
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        let mut deps = Vec::new();
        deps.extend(self.dep_primary.dependencies());
        deps.extend(self.dep_secondary.dependencies());
        deps
    }

    fn get_watched(&self) -> Vec<camino::Utf8PathBuf> {
        vec![]
    }

    fn execute(
        &self,
        context: &TaskContext<G>,
        _: &mut Store,
        dependencies: &[Dynamic],
        old_output: Option<&Dynamic>,
        updated_nodes: &HashSet<NodeIndex>,
    ) -> anyhow::Result<Map<Self::Output>> {
        // We assume the first dependency is the primary Many<T>
        let input_map = dependencies[0].downcast_ref::<Map<T>>().unwrap();

        let mut forced_dirty = false;
        for dep_idx in self.dep_secondary.dependencies() {
            if updated_nodes.contains(&dep_idx) {
                forced_dirty = true;
                break;
            }
        }

        let old_map = if !forced_dirty {
            old_output.and_then(|d| d.downcast_ref::<Map<Self::Output>>())
        } else {
            None
        };

        let mut result_map = BTreeMap::new();
        for (key, (input, provenance)) in &input_map.map {
            // If not forced dirty, and we have old output, and provenance matches
            if let Some(old_map) = old_map
                && let Some((old_item, old_provenance)) = old_map.map.get(key)
                && old_provenance == provenance
            {
                result_map.insert(key.clone(), (old_item.clone(), *provenance));
            } else {
                let (_, deps) = self.dep_secondary.resolve(&dependencies[1..]);
                let output = (self.callback)(context, input, deps)?;
                result_map.insert(key.clone(), (output, *provenance));
            }
        }

        Ok(Map {
            map: result_map,
            dirty: forced_dirty,
        })
    }

    fn is_valid(
        &self,
        _old_tracking: &[Option<TrackerState>],
        _new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool {
        for dep in self.dep_primary.dependencies() {
            if updated_nodes.contains(&dep) {
                return false;
            }
        }

        for dep in self.dep_secondary.dependencies() {
            if updated_nodes.contains(&dep) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::Environment;
    use crate::core::{Dynamic, Hash32, ImportMap, Store, TaskContext};
    use crate::engine::{
        Many, Map, One, Provenance, TrackerState, Tracking, TypedCoarse, TypedFine,
    };

    use std::borrow::Cow;
    use std::collections::{BTreeMap, HashSet};
    use std::marker::PhantomData;
    use std::sync::Arc;

    use petgraph::graph::NodeIndex;

    const ENV: Environment = Environment {
        generator: "test",
        mode: crate::core::Mode::Build,
        port: None,
        data: (),
    };

    // --- Helpers ---

    fn make_ctx() -> TaskContext<'static, ()> {
        TaskContext {
            env: &ENV,
            importmap: Box::leak(Box::new(ImportMap::new())),
            span: tracing::Span::none(),
        }
    }

    fn make_fine_output(items: Vec<(&str, i32, u32)>) -> Dynamic {
        let mut map = BTreeMap::new();
        for (k, v, hash) in items {
            map.insert(k.into(), (v, Provenance(Hash32::hash(hash.to_ne_bytes()))));
        }
        Arc::new(Map { map, dirty: false })
    }

    fn make_coarse_output(val: i32) -> Dynamic {
        Arc::new(val)
    }

    fn extract_state(tracking: Tracking) -> Option<TrackerState> {
        // Tracking::unwrap returns Vec<Option<TrackerState>>
        match tracking.unwrap().as_slice() {
            [Some(state)] => Some(state.clone()),
            _ => None,
        }
    }

    // --- NodeGather Tests ---

    #[test]
    fn test_gather_selective_access() {
        let dep_idx = NodeIndex::new(1);
        let node = NodeGather {
            name: Cow::Borrowed("reader"),
            dependencies: Many::<i32>::new(dep_idx),
            _phantom: PhantomData::<()>,
            callback: |_, tracker| {
                let _ = tracker.get("file_a").ok();
                Ok(())
            },
        };

        let input_v1 = make_fine_output(vec![("file_a", 100, 1), ("file_b", 200, 1)]);
        let (tracking, _) = node
            .execute(&make_ctx(), &mut Store::new(), &[input_v1])
            .unwrap();
        let state = extract_state(tracking).expect("Should have tracking state");

        // 1. Unread file changes -> Valid
        let input_v2 = make_fine_output(vec![("file_a", 100, 1), ("file_b", 999, 2)]);
        let updated_nodes: HashSet<_> = vec![dep_idx].into_iter().collect();
        assert!(
            node.is_valid(&[Some(state.clone())], &[input_v2], &updated_nodes),
            "Should be valid if unread file changes"
        );

        // 2. Read file changes -> Invalid
        let input_v3 = make_fine_output(vec![("file_a", 100, 2), ("file_b", 200, 1)]);
        assert!(
            !node.is_valid(&[Some(state)], &[input_v3], &updated_nodes),
            "Should be invalid if read file changes"
        );
    }

    #[test]
    fn test_gather_iteration() {
        let dep_idx = NodeIndex::new(1);
        let node = NodeGather {
            name: Cow::Borrowed("iter"),
            dependencies: Many::<i32>::new(dep_idx),
            _phantom: PhantomData::<()>,
            callback: |_, tracker| {
                for _ in tracker.iter() {}
                Ok(())
            },
        };

        let input_v1 = make_fine_output(vec![("a", 1, 1)]);
        let (tracking, _) = node
            .execute(&make_ctx(), &mut Store::new(), &[input_v1])
            .unwrap();
        let state = extract_state(tracking).unwrap();
        let updated_nodes: HashSet<_> = vec![dep_idx].into_iter().collect();

        // New file added -> Invalid
        let input_v2 = make_fine_output(vec![("a", 1, 1), ("b", 2, 1)]);
        assert!(
            !node.is_valid(&[Some(state)], &[input_v2], &updated_nodes),
            "Should be invalid if new file added"
        );
    }

    #[test]
    fn test_gather_globs() {
        let dep_idx = NodeIndex::new(1);
        let node = NodeGather {
            name: Cow::Borrowed("glob"),
            dependencies: Many::<i32>::new(dep_idx),
            _phantom: PhantomData::<()>,
            callback: |_, tracker| {
                for _ in tracker.glob("*.txt").unwrap() {}
                Ok(())
            },
        };

        let input_v1 = make_fine_output(vec![("a.txt", 1, 1), ("b.png", 2, 1)]);
        let (tracking, _) = node
            .execute(&make_ctx(), &mut Store::new(), &[input_v1])
            .unwrap();
        let state = extract_state(tracking).unwrap();
        let updated_nodes: HashSet<_> = vec![dep_idx].into_iter().collect();

        // 1. Non-matching file changes -> Valid
        let input_v2 = make_fine_output(vec![("a.txt", 1, 1), ("b.png", 99, 2)]);
        assert!(
            node.is_valid(&[Some(state.clone())], &[input_v2], &updated_nodes),
            "Should be valid if non-matching file changes"
        );

        // 2. Matching file changes -> Invalid
        let input_v3 = make_fine_output(vec![("a.txt", 1, 2), ("b.png", 2, 1)]);
        assert!(
            !node.is_valid(&[Some(state)], &[input_v3], &updated_nodes),
            "Should be invalid if matching file changes"
        );
    }

    #[test]
    fn test_gather_coarse_dep() {
        let dep_idx = NodeIndex::new(1);
        let node = NodeGather {
            name: Cow::Borrowed("coarse"),
            dependencies: One::<i32>::new(dep_idx),
            _phantom: PhantomData::<()>,
            callback: |_, _| Ok(()),
        };

        let input = make_coarse_output(1);
        let (tracking, _) = node
            .execute(&make_ctx(), &mut Store::new(), std::slice::from_ref(&input))
            .unwrap();
        let state = extract_state(tracking); // Should be None for One dependency

        let updated_nodes: HashSet<_> = vec![dep_idx].into_iter().collect();
        // Upstream changed -> Invalid
        assert!(
            !node.is_valid(&[state], &[input], &updated_nodes),
            "Should be invalid if upstream coarse node changed"
        );
    }

    // --- NodeScatter Tests ---

    #[test]
    fn test_scatter_invalidation() {
        let dep_idx = NodeIndex::new(1);
        let node = NodeScatter {
            name: Cow::Borrowed("scatter"),
            dependencies: One::<i32>::new(dep_idx),
            _phantom: PhantomData::<()>,
            callback: |_, input| Ok(vec![("key".into(), *input)]),
        };

        let input = make_coarse_output(10);
        let updated_nodes: HashSet<_> = vec![dep_idx].into_iter().collect();
        // Always invalid if dependency updated
        assert!(
            !node.is_valid(&[], &[input], &updated_nodes),
            "Scatter should be invalid if dependency changed"
        );
    }

    // --- NodeMap Tests ---

    #[test]
    fn test_map_reuse() {
        let prim_idx = NodeIndex::new(1);
        let sec_idx = NodeIndex::new(2);

        // NodeMap: Primary (Many<i32>) + Secondary (One<i32>)
        // Callback adds secondary val to primary val
        let node = NodeMap {
            name: Cow::Borrowed("map"),
            dep_primary: Many::<i32>::new(prim_idx),
            dep_secondary: One::<i32>::new(sec_idx),
            _phantom: PhantomData::<()>,
            callback: |_, prim, sec| Ok(*prim + *sec),
        };

        let input_prim = make_fine_output(vec![("a", 10, 1), ("b", 20, 1)]);
        let input_sec = make_coarse_output(5);
        let inputs = vec![input_prim.clone(), input_sec.clone()];

        // 1. First Execution
        let out_v1 = node
            .execute(
                &make_ctx(),
                &mut Store::new(),
                &inputs,
                None,
                &HashSet::new(),
            )
            .unwrap();

        let val_a = out_v1.map.get("a").unwrap().0;
        assert_eq!(val_a, 15); // 10 + 5

        // 2. Incremental Reuse: "a" unchanged, "b" changed
        let input_prim_v2 = make_fine_output(vec![("a", 10, 1), ("b", 30, 2)]);
        let inputs_v2 = vec![input_prim_v2, input_sec];

        // We pass out_v1 as old_output
        let old_dynamic: Dynamic = Arc::new(out_v1);
        let out_v2 = node
            .execute(
                &make_ctx(),
                &mut Store::new(),
                &inputs_v2,
                Some(&old_dynamic),
                &HashSet::new(),
            )
            .unwrap();

        assert!(!out_v2.dirty);
        assert_eq!(out_v2.map.get("a").unwrap().0, 15);
        assert_eq!(out_v2.map.get("b").unwrap().0, 35); // 30 + 5
    }

    #[test]
    fn test_map_secondary_forced_dirty() {
        let prim_idx = NodeIndex::new(1);
        let sec_idx = NodeIndex::new(2);

        let node = NodeMap {
            name: Cow::Borrowed("map_dirty"),
            dep_primary: Many::<i32>::new(prim_idx),
            dep_secondary: One::<i32>::new(sec_idx),
            _phantom: PhantomData::<()>,
            callback: |_, prim, sec| Ok(*prim + *sec),
        };

        // Initial Run
        let input_prim = make_fine_output(vec![("a", 10, 1)]);
        let input_sec_v1 = make_coarse_output(5);
        let inputs_v1 = vec![input_prim.clone(), input_sec_v1];

        let out_v1 = node
            .execute(
                &make_ctx(),
                &mut Store::new(),
                &inputs_v1,
                None,
                &HashSet::new(),
            )
            .unwrap();
        let old_dynamic: Dynamic = Arc::new(out_v1);

        // Secondary dependency changes -> 10
        let input_sec_v2 = make_coarse_output(10);
        let inputs_v2 = vec![input_prim, input_sec_v2];
        let updated_nodes: HashSet<_> = vec![sec_idx].into_iter().collect();

        // 1. is_valid should return false because secondary updated
        assert!(
            !node.is_valid(&[], &[], &updated_nodes),
            "NodeMap should be invalid if secondary dependency updated"
        );

        // 2. execute should force dirty and recompute EVERYTHING
        let out_v2 = node
            .execute(
                &make_ctx(),
                &mut Store::new(),
                &inputs_v2,
                Some(&old_dynamic),
                &updated_nodes,
            )
            .unwrap();

        assert!(
            out_v2.dirty,
            "Map should be marked dirty due to forced update"
        );
        assert_eq!(out_v2.map.get("a").unwrap().0, 20); // 10 + 10 (recomputed)
    }
}
