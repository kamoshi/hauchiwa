//! All the generic task-related abstractions.
use petgraph::graph::NodeIndex;
use std::any::Any;
use std::sync::Arc;

pub type Dynamic = Arc<dyn Any + Send + Sync>;

/// A handle to a task in the task graph.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Handle<T> {
    pub(crate) index: NodeIndex,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> Handle<T> {
    pub(crate) fn new(index: NodeIndex) -> Self {
        Self {
            index,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn index(&self) -> NodeIndex {
        self.index
    }
}

/// A trait that represents the dependencies of a task.
pub trait TaskDependencies {
    /// The type of the resolved dependencies. For a tuple of `Handle<T>`s, this will be a tuple of `T`s.
    type Output<'a>;

    /// Returns the `NodeIndex`s of the dependencies.
    fn dependencies(&self) -> Vec<NodeIndex>;

    /// Resolves the dependencies' outputs from a slice of `Dynamic`s.
    /// The order of the `Dynamic`s must match the order of the dependencies.
    /// # Panics
    /// This function may panic if the `Dynamic`s cannot be downcast to the expected types.
    fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> Self::Output<'a>;
}

impl TaskDependencies for () {
    type Output<'a> = ();

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn resolve<'a>(&self, _outputs: &'a [Dynamic]) -> Self::Output<'a> {
        ()
    }
}

macro_rules! impl_deps {
    ($($T:ident),*) => {
        #[allow(non_snake_case)]
        impl<$($T: Send + Sync + 'static),*> TaskDependencies for ($(Handle<$T>,)*) {
            type Output<'a> = ($(&'a $T,)*);

            fn dependencies(&self) -> Vec<NodeIndex> {
                let ($($T,)*) = self;
                vec![$($T.index),*]
            }



            fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> Self::Output<'a> {
                let mut iter = outputs.iter();
                ($({
                    let out = iter.next().unwrap();
                    out.downcast_ref::<$T>().unwrap()
                },)*)
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
