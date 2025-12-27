#![deny(
    unsafe_code,
    // clippy::unwrap_used,
    // clippy::expect_used,
    clippy::panic,
)]

pub mod error;
mod executor;
pub mod importmap;
pub mod loader;
pub mod page;
pub mod task;
mod utils;

pub use camino;

use std::{any::type_name, fmt::Debug, sync::Arc};

use camino::Utf8PathBuf;
use petgraph::{Graph, graph::NodeIndex};
use task::TaskDependencies;

#[deprecated = "Use hauchiwa::gitscan instead"]
pub use gitscan as gitmap;
pub use gitscan;

use crate::{
    importmap::ImportMap,
    loader::Store,
    task::{Dynamic, Task, TypedTask},
};

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

/// The mode in which the site generator is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A one-time build.
    Build,
    /// A continuous watch mode for development.
    Watch,
}

/// Global configuration and state available to all tasks.
///
/// This struct allows you to share global data (like configuration options or
/// shared state) across your entire task graph.
///
/// # Type Parameters
///
/// * `G`: The type of the user-defined global data. Must be `Send + Sync`.
#[derive(Clone)]
pub struct Environment<D: Send + Sync = ()> {
    /// The name of the generator (defaults to "hauchiwa").
    pub generator: &'static str,
    /// The current build mode (Build or Watch).
    pub mode: Mode,
    /// The port of the development server (if running).
    pub port: Option<u16>,
    /// User-defined global data.
    pub data: D,
}

impl<G: Send + Sync> Environment<G> {
    /// Returns a JavaScript snippet to enable live-reloading.
    ///
    /// If the site is running in `Watch` mode and a port is configured, this returns
    /// a script that connects to the WebSocket server to listen for reload events.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let script = ctx.globals.get_refresh_script();
    /// if let Some(s) = script {
    ///     // Inject `s` into your HTML <head> or <body>
    /// }
    /// ```
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

/// The context passed to every task execution.
///
/// `TaskContext` provides access to global settings and the aggregated import
/// map from all dependencies. It is immutable during task execution.
pub struct TaskContext<'a, G: Send + Sync = ()> {
    /// Access to global configuration and data.
    pub env: &'a Environment<G>,
    /// The current import map, containing JavaScript module mappings from all
    /// upstream dependencies.
    pub importmap: &'a ImportMap,
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
    F: for<'a> Fn(&TaskContext<'a, G>, D::Output<'a>) -> anyhow::Result<R> + Send + Sync,
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
    F: for<'a> Fn(&TaskContext<'a, G>, D::Output<'a>) -> anyhow::Result<R> + Send + Sync + 'static,
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
        context: &TaskContext<G>,
        _: &mut Store,
        dependencies: &[Dynamic],
    ) -> anyhow::Result<Self::Output> {
        let dependencies = self.dependencies.resolve(dependencies);
        (self.callback)(context, dependencies)
    }
}

/// The blueprint for your static site.
///
/// `Blueprint` is used to define the Task graph of your website. You add tasks
/// (including loaders) to the config, and wire them together using their
/// [Handle](crate::task::Handle)s.
///
/// Once configured, you convert this into a [Site] to execute the build.
///
/// # Example
///
/// ```rust,no_run
/// use hauchiwa::Blueprint;
///
/// let mut config: Blueprint<()> = Blueprint::new();
/// // Add tasks here...
/// ```
pub struct Blueprint<G: Send + Sync = ()> {
    graph: Graph<Arc<dyn Task<G>>, ()>,
}

impl<G: Send + Sync + 'static> Blueprint<G> {
    /// Creates a new, empty configuration.
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
        }
    }

    pub fn finish(self) -> Website<G> {
        Website { graph: self.graph }
    }

    /// Adds a custom task to the graph.
    ///
    /// This is the low-level method for adding tasks. For a more ergonomic
    /// experience, consider using the [`task!`](crate::task!) macro.
    ///
    /// # Arguments
    ///
    /// * `dependencies` - A tuple of handles to tasks that must run before this one.
    /// * `callback` - The closure that executes the task. It receives the
    ///   `Context` and the resolved outputs of the dependencies.
    ///
    /// # Returns
    ///
    /// A [`Handle`](crate::task::Handle) representing the future result of this task.
    pub fn add_task<D, F, R>(&mut self, dependencies: D, callback: F) -> task::Handle<R>
    where
        D: TaskDependencies + Send + Sync + 'static,
        F: for<'a> Fn(&TaskContext<'a, G>, D::Output<'a>) -> anyhow::Result<R>
            + Send
            + Sync
            + 'static,
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

impl<G: Send + Sync + 'static> Default for Blueprint<G> {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents the configured site and provides methods for building and serving
/// it with a development server.
///
/// A `Website` is created from a `Blueprint` and is the primary interface for
/// executing the build process.
pub struct Website<G: Send + Sync = ()> {
    graph: Graph<Arc<dyn Task<G>>, ()>,
}

impl<G> Website<G>
where
    G: Send + Sync + 'static,
{
    pub fn design() -> Blueprint<G> {
        Blueprint::default()
    }

    /// Runs the build process once.
    ///
    /// This will:
    /// 1. Clean the `dist` directory.
    /// 2. Copy static files.
    /// 3. Execute the task graph in parallel.
    /// 4. Save the generated `Page`s to `dist`.
    ///
    /// # Arguments
    ///
    /// * `data` - The global user data to pass to all tasks.
    pub fn build(&mut self, data: G) -> anyhow::Result<()> {
        let globals = Environment {
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

    /// Starts the development server in watch mode.
    ///
    /// This will perform an initial build and then watch for file changes.
    /// When a file changes, only the affected tasks are re-run.
    ///
    /// # Arguments
    ///
    /// * `data` - The global user data to pass to all tasks.
    pub fn watch(&mut self, data: G) -> anyhow::Result<()> {
        utils::clear_dist().expect("Failed to clear dist directory");
        utils::clone_static().expect("Failed to copy static files");

        crate::executor::watch(self, data)?;

        Ok(())
    }
}

/// A convenient macro for defining tasks.
///
/// This macro wraps `SiteConfig::add_task` to reduce boilerplate when
/// extracting dependencies.
///
/// # Syntax
///
/// ```rust,ignore
/// task!(config, |context, dep1, dep2| {
///     // body
/// })
/// ```
///
/// # Example
///
/// ```rust,no_run
/// # use hauchiwa::{Blueprint, task};
/// # let mut config: Blueprint<()> = Blueprint::new();
/// // Assume `dep_a` and `dep_b` are Handles from previous tasks.
/// // let dep_a = ...;
/// // let dep_b = ...;
///
/// # let dep_a = config.add_task((), |_, _| Ok(()));
/// # let dep_b = config.add_task((), |_, _| Ok(()));
///
/// task!(config, |ctx, dep_a, dep_b| {
///     // `dep_a` and `dep_b` here are the *results* of the tasks, not the handles.
///     println!("Task running!");
///     Ok(())
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
