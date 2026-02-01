//! All the generic task graph related abstractions.
//!
//! The Task system is the core of Hauchiwa. A [Task] is a unit of work that
//! produces a result. Tasks are organized into a Directed Acyclic Graph (DAG),
//! where dependencies are explicitly declared.
//!
//! ## Core abstractions
//!
//! * [`Handle<T>`]: A lightweight token representing the *future* result of a
//!   task. It is used to wire dependencies between tasks in the blueprint.
//! * [`TaskDependencies`]: A trait implemented for tuples of handles (e.g.,
//!   `(Handle<A>, Handle<B>)`). It auto-magically resolves these tokens into
//!   their concrete values `(&A, &B)` before executing the task.
//!
//! ## Phantom handles
//!
//! Under the hood, the graph is entirely type-erased. It stores all outputs as
//! `Arc<dyn Any + Send + Sync>`.
//!
//! We use a **phantom handle** to bridge this gap:
//! * **Compile-time**: `Handle<T>` carries no data but holds the type `T` in
//!   `PhantomData`. This allows the compiler to enforce that Task B receives
//!   exactly the type Task A produces.
//! * **Runtime**: The `TaskDependencies` trait performs the necessary `downcast_ref`
//!   logic. It acts as the safe bridge, panicking only if the strictly-typed
//!   blueprint construction was somehow bypassed (which the compiler prevents).

use std::collections::{HashMap, HashSet};

use crate::{
    engine::{Dynamic, Provenance},
    importmap::ImportMap,
};

/// Represents the data stored in the graph for each node.
/// Includes the user's output and the concatenated import map.
#[derive(Clone, Debug)]
pub(crate) struct NodeData {
    pub output: Dynamic,
    pub tracking: Vec<Option<HashMap<String, Provenance>>>,
    pub importmap: ImportMap,
}
