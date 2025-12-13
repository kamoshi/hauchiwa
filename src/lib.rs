#![deny(
    unsafe_code,
    // clippy::unwrap_used,
    // clippy::expect_used,
    clippy::panic,
)]

pub mod error;
mod executor;
pub mod gitmap;
pub mod loader;
pub mod page;
pub mod task;
mod utils;

pub use camino;

use std::{
    any::{Any, type_name},
    fmt::Debug,
    sync::Arc,
};

use camino::Utf8PathBuf;
use petgraph::{Graph, graph::NodeIndex};
use task::TaskDependencies;

use crate::task::{Task, TypedTask};

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

#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Build,
    Watch,
}

/// Represents globally accessible data and settings available to all tasks during the build process.
///
/// This struct holds information such as the project's generator name, the current build mode (build or watch),
/// and any user-defined data `G` that needs to be shared across different tasks.
#[derive(Debug, Clone)]
pub struct Globals<G: Send + Sync = ()> {
    /// The name and version of the generator.
    pub generator: &'static str,
    /// The current build mode, indicating whether the site is being built for production or watched for development.
    pub mode: Mode,
    /// The port for the live-reload WebSocket server, if applicable.
    pub port: Option<u16>,
    /// User-defined global data that can be accessed by tasks.
    pub data: G,
}

impl<G: Send + Sync> Globals<G> {
    /// Returns an inline JavaScript snippet for live-reloading the page in watch mode.
    ///
    /// If the site is running in watch mode, this function provides a script
    /// that establishes a WebSocket connection to the development server.
    /// A `None` value indicates that live-reload is disabled.
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

struct TaskNode<G, R, D, F>
where
    G: Send + Sync,
    R: Send + Sync + 'static,
    D: TaskDependencies,
    F: for<'a> Fn(&Globals<G>, D::Output<'a>) -> anyhow::Result<R> + Send + Sync,
{
    name: &'static str,
    dependencies: D,
    callback: F,
    _phantom: std::marker::PhantomData<G>,
}

impl<G, R, D, F> TypedTask<G> for TaskNode<G, R, D, F>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
    D: TaskDependencies + Send + Sync,
    F: for<'a> Fn(&Globals<G>, D::Output<'a>) -> anyhow::Result<R> + Send + Sync + 'static,
{
    type Output = R;

    fn get_name(&self) -> String {
        self.name.to_string()
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        self.dependencies.dependencies()
    }

    fn execute(
        &self,
        globals: &Globals<G>,
        dependencies: &[Dynamic],
    ) -> anyhow::Result<Self::Output> {
        let dependencies = self.dependencies.resolve(dependencies);
        (self.callback)(globals, dependencies)
    }
}

/// Configures and builds the task graph for a site.
///
/// `SiteConfig` is the main entry point for defining tasks and their dependencies.
/// After all tasks are added, it is used to create a `Site` instance.
pub struct SiteConfig<G: Send + Sync = ()> {
    graph: Graph<Arc<dyn Task<G>>, ()>,
}

impl<G: Send + Sync + 'static> SiteConfig<G> {
    /// Creates a new, empty `SiteConfig`.
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
        }
    }

    /// Adds a new task to the build graph.
    ///
    /// # Type Parameters
    ///
    /// - `D`: A tuple of `Handle<T>`s representing the task's dependencies.
    /// - `F`: The task's execution logic, provided as a closure.
    /// - `R`: The return type of the task's closure.
    ///
    /// # Parameters
    ///
    /// - `dependencies`: A tuple of handles to the tasks that must be completed before this one.
    /// - `callback`: A closure that takes `Globals` and the resolved outputs of the dependencies.
    ///
    /// # Returns
    ///
    /// A `Handle<R>` that can be used as a dependency for other tasks.
    pub fn add_task<D, F, R>(&mut self, dependencies: D, callback: F) -> task::Handle<R>
    where
        D: TaskDependencies + Send + Sync + 'static,
        F: for<'a> Fn(&Globals<G>, D::Output<'a>) -> anyhow::Result<R> + Send + Sync + 'static,
        R: Send + Sync + 'static,
    {
        self.add_task_opaque(TaskNode {
            name: type_name::<F>(),
            dependencies,
            callback,
            _phantom: std::marker::PhantomData,
        })
    }

    pub(crate) fn add_task_opaque<O, T>(&mut self, task: T) -> task::Handle<O>
    where
        O: 'static,
        T: TypedTask<G, Output = O> + 'static,
    {
        let dependencies = task.dependencies();
        let index = self.graph.add_node(Arc::new(task));

        for dependency in dependencies {
            self.graph.add_edge(dependency, index, ());
        }

        task::Handle::new(index)
    }
}

/// Represents the configured site and provides methods for building and serving it.
///
/// A `Site` is created from a `SiteConfig` and is the primary interface for
/// executing the build process.
pub struct Site<G: Send + Sync = ()> {
    graph: Graph<Arc<dyn Task<G>>, ()>,
}

impl<G: Send + Sync> Site<G> {
    /// Creates a new `Site` from a `SiteConfig`.
    ///
    /// This method consumes the configuration and prepares the site for execution.
    pub fn new(config: SiteConfig<G>) -> Self {
        Self {
            graph: config.graph,
        }
    }

    /// Executes a one-time build of the site.
    ///
    /// This runs all defined tasks in the correct order, handling dependencies,
    /// and outputs the resulting files to the `dist` directory.
    pub fn build(&mut self, data: G) -> anyhow::Result<()> {
        let globals = Globals {
            generator: "hauchiwa",
            mode: Mode::Build,
            port: None,
            data,
        };

        utils::clear_dist().expect("Failed to clear dist directory");
        utils::clone_static().expect("Failed to copy static files");

        let (_, pages) = crate::executor::run_once_parallel(self, &globals)?;

        crate::page::save_pages_to_dist(&pages).expect("Failed to save pages");

        Ok(())
    }

    /// Starts a development server and watches for file changes.
    ///
    /// This method performs an initial build and then monitors the project directory.
    /// When a file is modified, it intelligently re-runs only the necessary tasks.
    /// It also includes live-reloading functionality for a smooth development experience.
    pub fn watch(&mut self, data: G) -> anyhow::Result<()> {
        utils::clear_dist().expect("Failed to clear dist directory");
        utils::clone_static().expect("Failed to copy static files");

        crate::executor::watch(self, data)?;

        Ok(())
    }
}

/// A declarative macro for conveniently adding tasks to a `SiteConfig`.
///
/// This macro simplifies the process of defining tasks and their dependencies,
/// reducing boilerplate code.
///
/// # Usage
///
/// ```rust,ignore
/// use hauchiwa::task;
/// task!(config, |ctx, dep1, dep2| {
///     // Task logic here
/// });
/// ```
#[macro_export]
macro_rules! task {
    ($config:expr, |$ctx:pat_param $(, $($dep:ident $( : $ty:ty )? ),* )? | $body:block) => {
        $config.add_task(
            ( $( $($dep),* )? ),
            |$ctx, ( $( $($dep),* )? )| {
                // For each `ident: Ty`, emit: `let _: Ty = ident;`
                $( $( $( let _: $ty = $dep; )? )* )?

                $body
            }
        )
    };
}
