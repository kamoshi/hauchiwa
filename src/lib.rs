mod error;
mod gitmap;
pub mod executor;
pub mod loader;
pub mod page;
pub mod task;

use std::{
    any::{Any, TypeId},
    fmt::Debug,
    future::Future,
    sync::{Arc, LazyLock},
};

use camino::Utf8PathBuf;
use petgraph::{graph::NodeIndex, Graph};
use task::TaskDependencies;

/// 32 bytes length generic hash
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
struct Hash32([u8; 32]);

impl<T> From<T> for Hash32
where
    T: Into<[u8; 32]>,
{
    fn from(value: T) -> Self {
        Hash32(value.into())
    }
}

impl Hash32 {
    fn hash(buffer: impl AsRef<[u8]>) -> Self {
        blake3::Hasher::new()
            .update(buffer.as_ref())
            .finalize()
            .into()
    }

    fn hash_file(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        Ok(blake3::Hasher::new()
            .update_mmap_rayon(path)?
            .finalize()
            .into())
    }

    fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut acc = vec![0u8; 64];

        for (i, &byte) in self.0.iter().enumerate() {
            acc[i * 2] = HEX[(byte >> 4) as usize];
            acc[i * 2 + 1] = HEX[(byte & 0xF) as usize];
        }

        String::from_utf8(acc).unwrap()
    }
}

impl Debug for Hash32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hash32({})", self.to_hex())
    }
}

type Dynamic = Arc<dyn Any + Send + Sync>;
type DynamicResult = Result<Dynamic, error::LazyAssetError>;

#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Build,
    Watch,
}

/// `G` represents any additional data that should be globally available during
/// the HTML rendering process. If no such data is needed, it can be substituted
/// with `()`.
#[derive(Debug, Clone)]
pub struct Globals<G: Send + Sync = ()> {
    /// Generator name and version.
    pub generator: &'static str,
    /// Generator mode.
    pub mode: Mode,
    /// Watch port
    pub port: Option<u16>,
    /// Any additional data.
    pub data: G,
}

#[derive(Debug)]
pub struct FileMetadata {
    pub file: Utf8PathBuf,
    pub area: Utf8PathBuf,
    pub info: Option<gitmap::GitInfo>,
}

struct Item {
    /// Type ID for the type contained by this item. This is how you can filter
    /// items without having to evalute and downcast lazy data.
    refl_type: TypeId,
    /// Type name just for diagnostics.
    refl_name: &'static str,
    /// Simple ID for the item, doesn't have to be unique. Either the file path
    /// or user provided static string, used for querying context.
    id: Box<str>,
    /// Hash for the file contents. In the case of assets loaded from multiple
    /// files, like bundled scripts or stylesheets this will be the hash of the
    /// entire bundle. It's used for checking which task needs to be redone.
    hash: Hash32,
    /// If the item can be traced back to a filesystem entry this will be filled.
    file: Option<Arc<FileMetadata>>,
    /// Item computed on demand, cached in memory.
    data: LazyLock<DynamicResult, Box<dyn (FnOnce() -> DynamicResult) + Send + Sync>>,
}

pub trait Task<G: Send + Sync = ()>: Send + Sync {
    fn dependencies(&self) -> Vec<NodeIndex>;
    fn execute(&self, globals: &Globals<G>, dependencies: &[Dynamic]) -> Dynamic;
    fn on_file_change(&mut self, _path: &camino::Utf8Path) -> bool {
        false
    }
}

struct TaskNode<G, D, F, O>
where
    G: Send + Sync,
    D: TaskDependencies,
    F: Fn(&Globals<G>, D::Output) -> O + Send + Sync,
    O: Send + Sync + 'static,
{
    dependencies: D,
    callback: F,
    _phantom: std::marker::PhantomData<G>,
}

impl<G, D, F, O> Task<G> for TaskNode<G, D, F, O>
where
    G: Send + Sync + 'static,
    D: TaskDependencies + Send + Sync,
    F: Fn(&Globals<G>, D::Output) -> O + Send + Sync + 'static,
    O: Clone + Send + Sync + 'static,
{
    fn dependencies(&self) -> Vec<NodeIndex> {
        self.dependencies.dependencies()
    }

    fn execute(&self, globals: &Globals<G>, dependencies: &[Dynamic]) -> Dynamic {
        let dependencies = self.dependencies.resolve(dependencies);
        let output = (self.callback)(globals, dependencies);
        Arc::new(output)
    }
}

/// A builder struct for creating a `Website` with specified settings.
pub struct SiteConfig<G: Send + Sync = ()> {
    graph: Graph<Box<dyn Task<G>>, ()>,
}

impl<G: Send + Sync + 'static> SiteConfig<G> {
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
        }
    }

    pub fn add_task<D, F, O>(
        &mut self,
        dependencies: D,
        callback: F,
    ) -> task::Handle<O>
    where
        D: TaskDependencies + Send + Sync + 'static,
        F: Fn(&Globals<G>, D::Output) -> O + Send + Sync + 'static,
        O: Clone + Send + Sync + 'static,
    {
        let task = TaskNode {
            dependencies,
            callback,
            _phantom: std::marker::PhantomData,
        };
        self.add_task_boxed(Box::new(task))
    }

    pub fn add_task_boxed<O: 'static>(
        &mut self,
        task: Box<dyn Task<G>>,
    ) -> task::Handle<O> {
        let dependencies = task.dependencies();
        let index = self.graph.add_node(task);

        for dependency in dependencies {
            self.graph.add_edge(dependency, index, ());
        }

        task::Handle::new(index)
    }
}

pub struct Site<G: Send + Sync = ()> {
    pub graph: Graph<Box<dyn Task<G>>, ()>,
}

impl<G: Send + Sync> Site<G> {
    pub fn new(config: SiteConfig<G>) -> Self {
        Self {
            graph: config.graph,
        }
    }
}
