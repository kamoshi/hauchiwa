use petgraph::graph::NodeIndex;

use crate::engine::TrackerPtr;

/// A type-safe reference to a task in the build graph.
///
/// A `Handle<T>` is a lightweight, copyable token that represents a future
/// result of type `T`. It is used to define dependencies between tasks. When
/// one task depends on another, it holds a handle to that dependency. The build
/// system ensures that the dependency is executed before the task that depends
/// on it.
///
/// # Diamond Dependencies
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

impl<T> super::Handle for HandleC<T>
where
    T: Send + Sync + 'static,
{
    type Output<'a> = &'a T;

    fn index(&self) -> NodeIndex {
        self.index
    }

    fn downcast<'a>(&self, output: &'a super::Dynamic) -> (Option<TrackerPtr>, Self::Output<'a>) {
        let output = output
            .downcast_ref::<T>()
            .expect("Type mismatch in dependency resolution");

        (None, output)
    }
}
