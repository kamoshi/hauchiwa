//! All the generic task-related abstractions.
use petgraph::graph::NodeIndex;
use std::any::Any;
use std::sync::Arc;

use crate::{Context, importmap::ImportMap, loader::Runtime};

pub(crate) type Dynamic = Arc<dyn Any + Send + Sync>;

/// Represents the data stored in the graph for each node.
/// Includes the user's output and the concatenated import map.
#[derive(Clone, Debug)]
pub struct NodeData {
    pub output: Dynamic,
    pub importmap: ImportMap,
}

pub(crate) trait TypedTask<G: Send + Sync = ()>: Send + Sync {
    /// The concrete output type of this task.
    type Output: Send + Sync + 'static;

    fn get_name(&self) -> String;
    fn dependencies(&self) -> Vec<NodeIndex>;
    fn execute(
        &self,
        context: &Context<G>,
        runtime: &mut Runtime,
        dependencies: &[Dynamic],
    ) -> anyhow::Result<Self::Output>;

    #[inline]
    fn is_dirty(&self, _: &camino::Utf8Path) -> bool {
        false
    }
}

pub(crate) trait Task<G: Send + Sync = ()>: Send + Sync {
    fn get_name(&self) -> String;
    fn dependencies(&self) -> Vec<NodeIndex>;
    fn execute(
        &self,
        context: &Context<G>,
        runtime: &mut Runtime,
        dependencies: &[Dynamic],
    ) -> anyhow::Result<Dynamic>;

    #[inline]
    fn is_dirty(&self, _: &camino::Utf8Path) -> bool {
        false
    }
}

// A blanket implementation to automatically bridge the two. This is where the
// type erasure actually happens.
impl<G, T> Task<G> for T
where
    G: Send + Sync,
    T: TypedTask<G> + 'static,
{
    fn get_name(&self) -> String {
        T::get_name(self)
    }
    fn dependencies(&self) -> Vec<NodeIndex> {
        T::dependencies(self)
    }

    fn execute(
        &self,
        context: &Context<G>,
        runtime: &mut Runtime,
        dependencies: &[Dynamic],
    ) -> anyhow::Result<Dynamic> {
        // Call the typed method, then erase the result.
        Ok(Arc::new(T::execute(self, context, runtime, dependencies)?))
    }

    fn is_dirty(&self, path: &camino::Utf8Path) -> bool {
        T::is_dirty(self, path)
    }
}

/// A type-safe reference to a task in the build graph.
///
/// A `Handle<T>` is a lightweight, copyable token that represents a future result of type `T`.
/// It is used to define dependencies between tasks. When one task depends on another, it holds
/// a handle to that dependency. The build system ensures that the dependency is executed
/// before the task that depends on it.
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

    /// Returns the underlying `NodeIndex` of the task in the graph.
    pub fn index(&self) -> NodeIndex {
        self.index
    }
}

/// A trait that enables a collection of `Handle<T>`s to be used as dependencies for a task.
///
/// This trait is implemented for tuples of `Handle<T>`s, allowing them to be passed
/// as the `dependencies` argument to `SiteConfig::add_task`. It provides the necessary logic
/// for the build system to extract dependency information and resolve their outputs.
pub trait TaskDependencies {
    /// The resulting type when all dependencies are resolved.
    /// For a tuple of `Handle<T>`s, this will be a tuple of `&'a T`s.
    type Output<'a>;

    /// Returns the `NodeIndex` for each dependency in the collection.
    fn dependencies(&self) -> Vec<NodeIndex>;

    /// Takes a slice of type-erased dependency outputs and resolves them into a concrete `Output` type.
    ///
    /// # Panics
    /// This method will panic if the type-erased outputs cannot be downcast to their expected concrete types,
    /// indicating a severe logic error in the build system.
    fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> Self::Output<'a>;
}

impl TaskDependencies for () {
    type Output<'a> = ();

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn resolve<'a>(&self, _outputs: &'a [Dynamic]) -> Self::Output<'a> {}
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
                    out.downcast_ref::<$T>().unwrap_or_else(|| {
                        panic!("Expected {} but got something else", std::any::type_name::<$T>())
                    })
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
