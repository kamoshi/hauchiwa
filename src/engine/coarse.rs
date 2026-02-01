use std::collections::HashSet;
use std::sync::Arc;

use petgraph::graph::NodeIndex;

use crate::core::Dynamic;
use crate::engine::Handle;
use crate::engine::tracking::{TrackerPtr, TrackerState, Tracking};

/// A "coarse" type-safe reference to a task in the build graph.
///
/// A `HandleC<T>` represents a dependency on the **entire** result of an
/// upstream task. Unlike granular dependencies (which track specific reads),
/// this operates on an "all-or-nothing" basis.
///
/// # Granularity
///
/// This handle provides direct access to the output as `&T`. Because it does
/// not record which specific parts of `T` were used, the build system takes a
/// conservative approach: if the upstream task is re-executed (is "dirty"), any
/// task holding a `HandleC` to it is automatically invalidated and forced to
/// re-run.
///
/// # Diamond dependencies
///
/// Handles are smart enough to handle "diamond dependencies". If Task C and
/// Task B both depend on Task A, and Task D depends on both B and C, Task A
/// will only be executed *once*, and its result will be shared.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct HandleC<T> {
    pub(crate) index: NodeIndex,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> HandleC<T> {
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

impl<T> Clone for HandleC<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for HandleC<T> {}

impl<T> Handle for HandleC<T>
where
    T: Send + Sync + 'static,
{
    type Output<'a> = &'a T;

    fn index(&self) -> NodeIndex {
        self.index
    }

    fn downcast<'a>(&self, output: &'a Dynamic) -> (Option<TrackerPtr>, Self::Output<'a>) {
        let output = output
            .downcast_ref::<T>()
            .expect("Type mismatch in dependency resolution");

        (None, output)
    }

    fn is_valid(
        &self,
        _: &Option<TrackerState>,
        _: &Dynamic,
        updated: &HashSet<NodeIndex>,
    ) -> bool {
        !updated.contains(&self.index)
    }
}

pub(crate) trait TypedCoarse<G: Send + Sync = ()>: Send + Sync {
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

    fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool;
}

pub(crate) trait Coarse<G: Send + Sync = ()>: Send + Sync {
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

    fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool;
}

impl<G, T> Coarse<G> for T
where
    G: Send + Sync,
    T: TypedCoarse<G> + 'static,
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

    fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool {
        T::is_valid(self, old_tracking, new_outputs, updated_nodes)
    }
}
