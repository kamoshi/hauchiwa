use std::sync::Arc;

use petgraph::graph::NodeIndex;

use crate::engine::{Dynamic, Tracking};

pub(crate) trait TypedTaskC<G: Send + Sync = ()>: Send + Sync {
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
    ) -> anyhow::Result<(Tracking, Self::Output)>;

    fn is_dirty(&self, _: &camino::Utf8Path) -> bool {
        false
    }
}

/// The core trait for all tasks in the graph.
///
/// While most users will interact with the typed [`Blueprint::add_task`](crate::Blueprint::add_task)
/// API, this trait is the type-erased foundation that allows the graph to hold
/// tasks with different output types.
pub(crate) trait TaskC<G: Send + Sync = ()>: Send + Sync {
    fn get_name(&self) -> String;

    fn get_output_type_name(&self) -> &'static str;

    fn is_output(&self) -> bool;

    fn dependencies(&self) -> Vec<NodeIndex>;

    fn get_watched(&self) -> Vec<camino::Utf8PathBuf>;

    fn execute(
        &self,
        context: &crate::TaskContext<G>,
        runtime: &mut crate::Store,
        dependencies: &[Dynamic],
    ) -> anyhow::Result<(Tracking, Dynamic)>;

    #[inline]
    fn is_dirty(&self, _: &camino::Utf8Path) -> bool {
        false
    }
}

// A blanket implementation to automatically bridge the two. This is where the
// type erasure actually happens.
impl<G, T> TaskC<G> for T
where
    G: Send + Sync,
    T: TypedTaskC<G> + 'static,
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
    ) -> anyhow::Result<(Tracking, Dynamic)> {
        // Call the typed method, then erase the result.
        let (tracking, output) = T::execute(self, context, runtime, dependencies)?;
        Ok((tracking, Arc::new(output)))
    }

    fn is_dirty(&self, path: &camino::Utf8Path) -> bool {
        T::is_dirty(self, path)
    }
}
