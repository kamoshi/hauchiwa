use std::collections::HashMap;
use std::any::Any;
use std::marker::PhantomData;
use std::path::PathBuf;
pub use petgraph::graph::{NodeIndex, Graph};
use petgraph::Directed;

/// The central graph. Edges are A -> B, meaning "A depends on B".
/// The graph only stores the dependency structure.
pub type DependencyGraph = Graph<(), (), Directed>;

/// A type-safe, copyable key that points to a node in the graph.
/// This is the user-facing "Handle".
/// The generic `T` is the *output type* of the node.
pub struct Handle<T> {
    pub(crate) index: NodeIndex,
    pub(crate) _marker: PhantomData<T>,
}

// We need this so users can pass handles around easily.
impl<T> Clone for Handle<T> { fn clone(&self) -> Self { *self } }
impl<T> Copy for Handle<T> {}

/// A type-alias for a collection's handle.
/// `T` is the type of the content, e.g., `Post`.
pub type CollectionHandle<T> = Handle<Vec<T>>; // A collection is a Vec<T>

/// A type-alias for a task's artifact handle.
/// `T` is the type of the artifact, e.g., `String` (for a URL).
pub type ArtifactHandle<T> = Handle<T>;

/// The new "Build Database".
/// This is the central, shared cache where all task results are stored.
/// It's a type-erased map from a node's ID to its output data.
#[derive(Default)]
pub struct Sack {
    /// Stores the *output* of every node (task or collection)
    /// Key: The NodeIndex of the task that produced the data.
    /// Value: The data itself, type-erased (e.g., `Box<Vec<Post>>`).
    data: HashMap<NodeIndex, Box<dyn Any + Send + Sync>>,
    /// Stores the hash of the output of every node.
    pub hashes: HashMap<NodeIndex, u64>,
}

impl Sack {
    /// Internal method for a runner to add its artifact.
    pub(crate) fn add_artifact(&mut self, index: NodeIndex, data: Box<dyn Any + Send + Sync>, hash: u64) {
        self.data.insert(index, data);
        self.hashes.insert(index, hash);
    }

    /// Internal method for a runner to resolve a dependency.
    pub(crate) fn get_data<T: 'static>(&self, index: NodeIndex) -> Option<&T> {
        self.data
            .get(&index)
            .and_then(|any_data| any_data.downcast_ref::<T>())
    }
}

/// The result of any task execution.
pub struct TaskOutput {
    /// A list of files to be written to disk for this task.
    pub files_to_write: Vec<(PathBuf, String)>,
    /// The optional artifact this task provides to other tasks.
    /// This will be stored in the Sack.
    pub artifact: Box<dyn Any + Send + Sync>,
    /// The hash of the artifact.
    pub hash: u64,
}

/// A type-erased "runner" for a node in the graph.
/// Each Collection and Task will be packaged into one of these.
pub trait TaskRunner: Send + Sync {
    /// The core execution method.
    /// It must:
    /// 1. Fetch its dependencies' data from the Sack.
    /// 2. Run the user's logic.
    /// 3. Return the files to write and its own artifact.
    fn run(&self, sack: &Sack) -> Result<TaskOutput, String>;

    /// A list of nodes this runner depends on.
    /// This is used for building the graph.
    fn get_dependencies(&self) -> Vec<NodeIndex>;
}

/// The main builder that the user interacts with.
pub struct WebsiteBuilder {
    /// The dependency graph.
    pub graph: DependencyGraph,

    /// Maps each NodeIndex to its executable logic.
    pub nodes: HashMap<NodeIndex, Box<dyn TaskRunner>>,

    /// Maps glob patterns to their NodeIndex.
    pub glob_map: HashMap<&'static str, NodeIndex>,
}

/// The final "built" website, ready to be executed.
pub struct Website {
    pub graph: DependencyGraph,
    pub nodes: HashMap<NodeIndex, Box<dyn TaskRunner>>,
    /// The full build order, pre-calculated.
    pub build_order: Vec<NodeIndex>,
    /// Maps glob patterns to their NodeIndex.
    pub glob_map: HashMap<&'static str, NodeIndex>,
}