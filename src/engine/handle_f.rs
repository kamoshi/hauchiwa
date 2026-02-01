use petgraph::graph::NodeIndex;

use crate::engine::{
    Dynamic,
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
}
