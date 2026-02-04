use std::borrow::Cow;
use std::collections::HashSet;
use std::hash::Hash;
use std::marker::PhantomData;

use petgraph::graph::NodeIndex;

use crate::core::{Blake3Hasher, Dynamic, Store, TaskContext};
use crate::engine::{
    Dependencies, Map, Provenance, TrackerState, Tracking, TypedCoarse, TypedFine,
};

pub(crate) struct TaskNodeCoarse<G, R, D, F>
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

impl<G, R, D, F> TypedCoarse<G> for TaskNodeCoarse<G, R, D, F>
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

pub(crate) struct TaskNodeFine<G, R, D, F>
where
    G: Send + Sync,
    R: Send + Sync + Hash + 'static,
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

impl<G, R, D, F> TypedFine<G> for TaskNodeFine<G, R, D, F>
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
