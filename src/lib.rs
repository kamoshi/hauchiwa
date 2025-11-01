pub mod error;
pub mod executor;
mod gitmap;
pub mod loader;
pub mod page;
pub mod task;

pub use camino;

use std::{any::Any, fmt::Debug, sync::Arc};

use camino::Utf8PathBuf;
use petgraph::{Graph, graph::NodeIndex};
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

impl<G: Send + Sync> Globals<G> {
    /// If live reload is enabled, returns an inline JavaScript snippet to
    /// establish a WebSocket connection for hot page refresh during
    /// development.
    pub fn get_refresh_script(&self) -> Option<String> {
        self.port.map(|port| {
            format!(
                r#"
const socket = new WebSocket("ws://localhost:{port}");
socket.addEventListener("message", event => {{
    window.location.reload();
}});
"#
            )
        })
    }
}

#[derive(Debug)]
pub struct FileMetadata {
    pub file: Utf8PathBuf,
    pub area: Utf8PathBuf,
    pub info: Option<gitmap::GitInfo>,
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
    F: for<'a> Fn(&Globals<G>, D::Output<'a>) -> O + Send + Sync,
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
    F: for<'a> Fn(&Globals<G>, D::Output<'a>) -> O + Send + Sync + 'static,
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

    pub fn add_task<D, F, R>(&mut self, dependencies: D, callback: F) -> task::Handle<R>
    where
        D: TaskDependencies + Send + Sync + 'static,
        F: for<'a> Fn(&Globals<G>, D::Output<'a>) -> R + Send + Sync + 'static,
        R: Clone + Send + Sync + 'static,
    {
        self.add_task_opaque(TaskNode {
            dependencies,
            callback,
            _phantom: std::marker::PhantomData,
        })
    }

    pub(crate) fn add_task_opaque<O: 'static, T: Task<G> + 'static>(
        &mut self,
        task: T,
    ) -> task::Handle<O> {
        let dependencies = task.dependencies();
        let index = self.graph.add_node(Box::new(task));

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
