mod handle_c;
mod handle_f;
mod task_c;
mod task_f;

use std::{any::Any, collections::HashSet, sync::Arc};

use petgraph::graph::NodeIndex;

pub use crate::engine::handle_c::HandleC;
pub use crate::engine::handle_f::HandleF;

pub(crate) use crate::engine::task_c::TypedTaskC;
pub(crate) use crate::engine::task_f::{Map, TrackerPtr, TypedTaskF, TrackerState};
pub use crate::engine::task_f::{Provenance, Tracker};

pub(crate) type Dynamic = Arc<dyn Any + Send + Sync>;

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

// Things that can be used as dependency Handle
pub trait Handle: Copy + Send + Sync {
    type Output<'a>;

    fn index(&self) -> NodeIndex;
    fn downcast<'a>(&self, output: &'a Dynamic) -> (Option<TrackerPtr>, Self::Output<'a>);
    fn is_valid(
        &self,
        old_tracking: &Option<TrackerState>,
        new_output: &Dynamic,
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool;
}

pub enum Task<G> {
    C(Arc<dyn task_c::TaskC<G>>),
    F(Arc<dyn task_f::TaskF<G>>),
}

impl<G> Task<G>
where
    G: Send + Sync,
{
    pub(crate) fn name(&self) -> String {
        match self {
            Task::C(task) => task.get_name(),
            Task::F(task) => task.get_name(),
        }
    }

    pub(crate) fn dependencies(&self) -> Vec<NodeIndex> {
        match self {
            Task::C(task) => task.dependencies(),
            Task::F(task) => task.dependencies(),
        }
    }

    pub(crate) fn watched(&self) -> Vec<camino::Utf8PathBuf> {
        match self {
            Task::C(task) => task.get_watched(),
            Task::F(task) => task.get_watched(),
        }
    }

    pub(crate) fn is_output(&self) -> bool {
        match self {
            Task::C(task) => task.is_output(),
            Task::F(task) => task.is_output(),
        }
    }

    pub(crate) fn type_name_output(&self) -> &'static str {
        match self {
            Task::C(task) => task.get_output_type_name(),
            Task::F(task) => task.get_output_type_name(),
        }
    }

    pub(crate) fn is_dirty(&self, path: &camino::Utf8Path) -> bool {
        match self {
            Task::C(task) => task.is_dirty(path),
            Task::F(task) => task.is_dirty(path),
        }
    }

    pub(crate) fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool {
        match self {
            Task::C(task) => task.is_valid(old_tracking, new_outputs, updated_nodes),
            Task::F(task) => task.is_valid(old_tracking, new_outputs, updated_nodes),
        }
    }
}

impl<G> Clone for Task<G> {
    fn clone(&self) -> Self {
        match self {
            Task::C(task) => Task::C(task.clone()),
            Task::F(task) => Task::F(task.clone()),
        }
    }
}

/// A trait that enables a collection of [`Handle<T>`]s to be used as
/// dependencies for a task.
///
/// This trait is implemented for tuples of [`Handle<T>`]s, allowing them to be
/// passed as the `dependencies` argument to `Blueprint::add_task`. It provides
/// the necessary logic for the build system to extract dependency information
/// and resolve their outputs.
pub trait Dependencies {
    /// The resulting type when all dependencies are resolved.
    /// For a tuple of [`Handle<T>`]s, this will be a tuple of `&'a T`s.
    type Output<'a>;

    /// Returns the [`NodeIndex`] for each dependency in the collection.
    fn dependencies(&self) -> Vec<NodeIndex>;

    /// Takes a slice of type-erased dependency outputs and resolves them into a
    /// concrete `Output` type.
    ///
    /// # Panics
    /// This method will panic if the type-erased outputs cannot be downcast to
    /// their expected concrete types, indicating a severe logic error in the
    /// build system.
    fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> (Tracking, Self::Output<'a>);

    fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool;
}

impl Dependencies for () {
    type Output<'a> = ();

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn resolve<'a>(&self, _: &'a [Dynamic]) -> (Tracking, Self::Output<'a>) {
        (Tracking::default(), ())
    }

    fn is_valid(
        &self,
        _: &[Option<TrackerState>],
        _: &[Dynamic],
        _: &HashSet<NodeIndex>,
    ) -> bool {
        true
    }
}

impl<H> Dependencies for H
where
    H: Handle,
{
    type Output<'a> = H::Output<'a>;

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![Handle::index(self)]
    }

    fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> (Tracking, Self::Output<'a>) {
        let mut tracking = Tracking::default();

        // self is the handle
        let (tracker_ptr, output) = self.downcast(&outputs[0]);
        tracking.edges.push(tracker_ptr);

        (tracking, output)
    }

    fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool {
        self.is_valid(&old_tracking[0], &new_outputs[0], updated_nodes)
    }
}

impl<H> Dependencies for Vec<H>
where
    H: Handle,
{
    type Output<'a> = Vec<H::Output<'a>>;

    fn dependencies(&self) -> Vec<NodeIndex> {
        self.iter().map(|h| h.index()).collect()
    }

    fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> (Tracking, Self::Output<'a>) {
        let mut tracking = Tracking::default();
        let mut result = Vec::with_capacity(self.len());

        for (handle, output) in self.iter().zip(outputs) {
            let (tracker_ptr, item) = handle.downcast(output);
            tracking.edges.push(tracker_ptr);
            result.push(item);
        }

        (tracking, result)
    }

    fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool {
        self.iter()
            .zip(old_tracking.iter())
            .zip(new_outputs.iter())
            .all(|((handle, tracking), output)| {
                handle.is_valid(tracking, output, updated_nodes)
            })
    }
}

macro_rules! impl_deps {
    ($($D:ident),*) => {
        #[allow(non_snake_case)]
        impl<$($D),*> Dependencies for ($($D,)*)
        where
            $($D: Handle),* {
            type Output<'a> = ($($D::Output<'a>,)*);

            fn dependencies(&self) -> Vec<NodeIndex> {
                let ($($D,)*) = self;
                vec![$(Handle::index($D),)*]
            }

            fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> (Tracking, Self::Output<'a>) {
                let mut tracking = Tracking::default();
                let ($($D,)*) = self;

                let mut iter = outputs.iter();

                let result = ($({
                    let out = iter.next().unwrap();
                    let (tracker_ptr, item) = $D.downcast(out);
                    tracking.edges.push(tracker_ptr);
                    item
                },)*);

                (tracking, result)
            }

            fn is_valid(
                &self,
                old_tracking: &[Option<TrackerState>],
                new_outputs: &[Dynamic],
                updated_nodes: &HashSet<NodeIndex>,
            ) -> bool {
                let ($($D,)*) = self;
                let mut tracking_iter = old_tracking.iter();
                let mut output_iter = new_outputs.iter();

                $(
                    if !$D.is_valid(tracking_iter.next().unwrap(), output_iter.next().unwrap(), updated_nodes) {
                        return false;
                    }
                )*

                true
            }
        }
    };
}

impl_deps!(A);
impl_deps!(A, B);
impl_deps!(A, B, C);
impl_deps!(A, B, C, D);
impl_deps!(A, B, C, D, E);
impl_deps!(A, B, C, D, E, F);
impl_deps!(A, B, C, D, E, F, G);
impl_deps!(A, B, C, D, E, F, G, H);
impl_deps!(A, B, C, D, E, F, G, H, I);
impl_deps!(A, B, C, D, E, F, G, H, I, J);
impl_deps!(A, B, C, D, E, F, G, H, I, J, K);
impl_deps!(A, B, C, D, E, F, G, H, I, J, K, L);
