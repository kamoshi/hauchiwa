use crate::core_structs::{Sack, Handle, NodeIndex};

/// A trait implemented by tuples of Handles (e.g., `(Handle<A>, Handle<B>)`).
/// This trait knows how to "resolve" itself from the Sack.
pub trait TaskDependencies: 'static + Send + Sync {
    /// The output type this tuple resolves to, e.g., `(A, B)`.
    type ResolvedData;

    /// The method that does the work of fetching and downcasting.
    fn resolve(&self, sack: &Sack) -> Result<Self::ResolvedData, String>;

    /// Returns the list of NodeIndexes this tuple depends on.
    fn get_indices(&self) -> Vec<NodeIndex>;
}

macro_rules! impl_task_dependencies {
    ($(($T:ident, $idx:tt)),+) => {
        impl<$($T),+> TaskDependencies for ($(Handle<$T>),+,)
        where
            $($T: 'static + Clone + Send + Sync),+
        {
            type ResolvedData = ($($T),+,);

            fn resolve(&self, sack: &Sack) -> Result<Self::ResolvedData, String> {
                Ok(($(
                    sack.get_data::<$T>(self.$idx.index)
                        .cloned()
                        .ok_or_else(|| format!("Failed to resolve dependency for node {:?}", self.$idx.index))?
                ),+,))
            }

            fn get_indices(&self) -> Vec<NodeIndex> {
                vec![$(self.$idx.index),+]
            }
        }
    };
}

impl_task_dependencies!((T1, 0));
impl_task_dependencies!((T1, 0), (T2, 1));
impl_task_dependencies!((T1, 0), (T2, 1), (T3, 2));
impl_task_dependencies!((T1, 0), (T2, 1), (T3, 2), (T4, 3));
impl_task_dependencies!((T1, 0), (T2, 1), (T3, 2), (T4, 3), (T5, 4));
impl_task_dependencies!((T1, 0), (T2, 1), (T3, 2), (T4, 3), (T5, 4), (T6, 5));
