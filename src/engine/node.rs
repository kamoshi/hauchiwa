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

        Ok(Map { map })
    }
}

/// Map each input to a single output, with additional (side) dependencies
/// Many<T> -> Many<R>
pub(crate) struct NodeMap<T, G, R, D, F>
where
    T: Send + Sync + 'static,
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
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
    R: Send + Sync + 'static,
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
    ) -> anyhow::Result<Map<Self::Output>> {
        // We assume the first dependency is the primary Many<T>
        let input_map = dependencies[0].downcast_ref::<Map<T>>().unwrap();

        let mut result_map = BTreeMap::new();
        for (key, (input, provenance)) in &input_map.map {
            let (_, deps) = self.dep_secondary.resolve(&dependencies[1..]);
            let output = (self.callback)(context, input, deps)?;
            result_map.insert(key.clone(), (output, *provenance));
        }

        Ok(Map { map: result_map })
    }
}
