mod handle_c;
mod handle_f;
mod task_c;
mod task_f;

use std::{any::Any, sync::Arc};

use petgraph::graph::NodeIndex;

pub use crate::engine::handle_c::HandleC;
pub use crate::engine::handle_f::HandleF;

pub(crate) use crate::engine::task_c::TypedTaskC;
pub use crate::engine::task_f::Tracker;
pub(crate) use crate::engine::task_f::{Map, TypedTaskF};

pub(crate) type Dynamic = Arc<dyn Any + Send + Sync>;

// Things that can be used as dependency Handle
pub trait Handle: Copy + Send + Sync {
    type Output<'a>;

    fn index(&self) -> NodeIndex;
    fn downcast<'a>(&self, output: &'a Dynamic) -> Self::Output<'a>;
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

    pub(crate) fn execute(
        &self,
        context: &crate::TaskContext<G>,
        runtime: &mut crate::Store,
        dependencies: &[Dynamic],
    ) -> anyhow::Result<Arc<dyn Any + Send + Sync>> {
        match self {
            Task::C(task) => task.execute(context, runtime, dependencies),
            Task::F(task) => task.execute(context, runtime, dependencies),
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
    fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> Self::Output<'a>;
}

impl Dependencies for () {
    type Output<'a> = ();

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn resolve<'a>(&self, _: &'a [Dynamic]) -> Self::Output<'a> {}
}

impl<H> Dependencies for H
where
    H: Handle,
{
    type Output<'a> = H::Output<'a>;

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![Handle::index(self)]
    }

    fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> Self::Output<'a> {
        // self is the handle
        self.downcast(&outputs[0])
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

    fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> Self::Output<'a> {
        let mut result = Vec::with_capacity(self.len());

        for (handle, output) in self.iter().zip(outputs) {
            let item = handle.downcast(output);
            result.push(item);
        }

        result
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

            fn resolve<'a>(&self, outputs: &'a [Dynamic]) -> Self::Output<'a> {
                let ($($D,)*) = self;

                let mut iter = outputs.iter();

                ($({
                    let out = iter.next().unwrap();
                    $D.downcast(out)
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
