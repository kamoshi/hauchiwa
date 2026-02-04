use std::any::type_name;
use std::borrow::Cow;
use std::marker::PhantomData;
use std::sync::Arc;

use petgraph::Graph;

use crate::core::{Environment, Mode, Store};
use crate::engine::{
    Dependencies, Many, NodeGather, NodeMap, NodeScatter, One, Task, TypedCoarse, TypedFine,
    run_once_parallel,
};
use crate::error::HauchiwaError;
use crate::loader::Input;
use crate::{Diagnostics, TaskContext};

/// The blueprint for your static site.
///
/// `Blueprint` is used to define the Task graph of your website. You add tasks
/// (including loaders) to the config, and wire them together using their
/// references like [`One`](crate::One) or [`Many`](crate::Many).
///
/// Once configured, you convert this into a [`Website`] to execute the build.
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
    pub(crate) graph: Graph<Task<G>, ()>,
}

impl<G: Send + Sync + 'static> Blueprint<G> {
    /// Creates a new, empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn task(&mut self) -> TaskDef<'_, G> {
        TaskDef {
            blueprint: self,
            name: None,
        }
    }

    pub fn finish(self) -> Website<G> {
        Website { graph: self.graph }
    }

    pub(crate) fn add_task_fine<O, T>(&mut self, task: T) -> Many<O>
    where
        O: 'static,
        T: TypedFine<G, Output = O> + 'static,
    {
        let dependencies = task.dependencies();
        let index = self.graph.add_node(Task::F(Arc::new(task)));

        for dependency in dependencies {
            self.graph.add_edge(dependency, index, ());
        }

        Many::new(index)
    }

    pub(crate) fn add_task_coarse<O, T>(&mut self, task: T) -> One<O>
    where
        O: 'static,
        T: TypedCoarse<G, Output = O> + 'static,
    {
        let dependencies = task.dependencies();
        let index = self.graph.add_node(Task::C(Arc::new(task)));

        for dependency in dependencies {
            self.graph.add_edge(dependency, index, ());
        }

        One::new(index)
    }
}

impl<G: Send + Sync> Default for Blueprint<G> {
    fn default() -> Self {
        Self {
            graph: Graph::new(),
        }
    }
}

impl<G> std::fmt::Display for Blueprint<G>
where
    G: Send + Sync + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "graph LR")?;

        for index in self.graph.node_indices() {
            let task = &self.graph[index];
            let name = task.name().replace('"', "\\\""); // Simple escape
            writeln!(f, "    {:?}[\"{}\"]", index.index(), name)?;

            if task.is_output() {
                writeln!(f, "    {:?} --> Output", index.index())?;
            }
        }

        writeln!(f, "    Output[Output]")?;

        for edge in self.graph.edge_indices() {
            let (source, target) = self.graph.edge_endpoints(edge).unwrap();
            let source_task = &self.graph[source];
            let type_name = source_task
                .type_name_output()
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            writeln!(
                f,
                "    {:?} -- \"{}\" --> {:?}",
                source.index(),
                type_name,
                target.index()
            )?;
        }

        Ok(())
    }
}

pub struct TaskDef<'a, G: Send + Sync> {
    blueprint: &'a mut Blueprint<G>,
    name: Option<Cow<'static, str>>,
}

impl<'a, G: Send + Sync + 'static> TaskDef<'a, G> {
    pub fn name(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Load assets from file system using glob pattern.
    pub fn glob(self, glob: impl Into<String>) -> TaskBinderGlob<'a, G> {
        TaskBinderGlob {
            blueprint: self.blueprint,
            name: self.name,
            sources: vec![glob.into()],
        }
    }

    /// Perform a map on a collection.
    pub fn each<T>(self, each: Many<T>) -> TaskBinderEach<'a, G, T, ()> {
        TaskBinderEach {
            blueprint: self.blueprint,
            name: self.name,
            primary: each,
            secondary: (),
        }
    }

    /// Set task dependencies.
    pub fn using<D>(self, dependencies: D) -> TaskBinder<'a, G, D>
    where
        D: Dependencies,
    {
        TaskBinder {
            blueprint: self.blueprint,
            name: self.name,
            dependencies,
        }
    }

    /// Immediately run a task with no dependencies.
    pub fn run<F, R>(self, callback: F) -> One<R>
    where
        F: Fn(&TaskContext<'_, G>) -> anyhow::Result<R> + Send + Sync + 'static,
        R: Send + Sync + 'static,
    {
        self.blueprint.add_task_coarse(NodeGather {
            name: self.name.unwrap_or(type_name::<F>().into()),
            dependencies: (),
            callback: move |ctx, ()| callback(ctx),
            _phantom: PhantomData,
        })
    }
}

pub struct TaskBinderGlob<'a, G: Send + Sync> {
    blueprint: &'a mut Blueprint<G>,
    name: Option<Cow<'static, str>>,
    sources: Vec<String>,
}

impl<'a, G: Send + Sync + 'static> TaskBinderGlob<'a, G> {
    pub fn name(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn glob(mut self, glob: impl Into<String>) -> Self {
        self.sources.push(glob.into());
        self
    }

    pub fn map<F, R>(self, callback: F) -> Result<Many<R>, HauchiwaError>
    where
        F: Fn(&TaskContext<G>, &mut Store, Input) -> anyhow::Result<R> + Send + Sync + 'static,
        R: Send + Sync + 'static,
    {
        let task = crate::loader::GlobFiles::new(
            self.sources.clone(),
            self.sources,
            move |ctx, store, input| {
                let path = input.path.clone();
                let res = callback(ctx, store, input)?;
                Ok((path, res))
            },
        )?;

        Ok(self.blueprint.add_task_fine(task))
    }
}

/// A binder for tasks that map over a collection with optional side dependencies.
pub struct TaskBinderEach<'a, G, T, D>
where
    G: Send + Sync,
{
    blueprint: &'a mut Blueprint<G>,
    name: Option<Cow<'static, str>>,
    primary: Many<T>,
    secondary: D,
}

impl<'a, G, T, D> TaskBinderEach<'a, G, T, D>
where
    G: Send + Sync + 'static,
    T: Send + Sync + 'static,
    D: Dependencies + Send + Sync + 'static,
{
    pub fn name(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Add secondary dependencies (context) that are needed for every item mapping.
    pub fn using<D2>(self, dependencies: D2) -> TaskBinderEach<'a, G, T, D2>
    where
        D2: Dependencies,
    {
        TaskBinderEach {
            blueprint: self.blueprint,
            name: self.name,
            primary: self.primary,
            secondary: dependencies,
        }
    }

    /// Execute the mapping function.
    /// The callback receives the context, the individual item `&T`, and the resolved secondary dependencies.
    pub fn map<F, R>(self, callback: F) -> Many<R>
    where
        F: for<'b> Fn(&TaskContext<'b, G>, &T, D::Output<'b>) -> anyhow::Result<R>
            + Send
            + Sync
            + 'static,
        R: Send + Sync + Clone + 'static,
    {
        self.blueprint.add_task_fine(NodeMap {
            name: self.name.unwrap_or(type_name::<F>().into()),
            dep_primary: self.primary,
            dep_secondary: self.secondary,
            callback,
            _phantom: PhantomData,
        })
    }
}

pub struct TaskBinder<'a, G: Send + Sync, D> {
    blueprint: &'a mut Blueprint<G>,
    name: Option<Cow<'static, str>>,
    dependencies: D,
}

impl<'a, G, D> TaskBinder<'a, G, D>
where
    G: Send + Sync + 'static,
    D: Dependencies + Send + Sync + 'static,
{
    pub fn name(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn merge<F, R>(self, callback: F) -> One<R>
    where
        F: for<'b> Fn(&TaskContext<'b, G>, D::Output<'b>) -> anyhow::Result<R>
            + Send
            + Sync
            + 'static,
        R: Send + Sync + 'static,
    {
        self.blueprint.add_task_coarse(NodeGather {
            name: self.name.unwrap_or(type_name::<F>().into()),
            dependencies: self.dependencies,
            callback,
            _phantom: PhantomData,
        })
    }

    pub fn spread<F, R>(self, callback: F) -> Many<R>
    where
        F: for<'b> Fn(&TaskContext<'b, G>, D::Output<'b>) -> anyhow::Result<Vec<(String, R)>>
            + Send
            + Sync
            + 'static,
        R: Send + Sync + std::hash::Hash + 'static,
    {
        self.blueprint.add_task_fine(NodeScatter {
            name: self.name.unwrap_or(type_name::<F>().into()),
            dependencies: self.dependencies,
            callback,
            _phantom: PhantomData,
        })
    }
}

/// Represents the configured site and provides methods for building and serving
/// it with a development server.
///
/// A [`Website`] is created from a [`Blueprint`] and is the primary interface
/// for executing the build process.
pub struct Website<G: Send + Sync = ()> {
    pub(crate) graph: Graph<Task<G>, ()>,
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
    /// 4. Save the generated [`Output`](crate::Output)s to `dist`.
    ///
    /// # Arguments
    ///
    /// * `data` - The global user data to pass to all tasks.
    pub fn build(&mut self, data: G) -> anyhow::Result<Diagnostics> {
        crate::utils::init_logging()?;

        let globals = Environment {
            generator: "hauchiwa",
            mode: Mode::Build,
            port: None,
            data,
        };

        crate::utils::clear_dist()?;
        crate::utils::clone_static()?;

        let (_, pages, diagnostics) = run_once_parallel(self, &globals)?;

        crate::output::save_pages_to_dist(&pages)?;

        Ok(diagnostics)
    }

    /// Starts the development server in watch mode.
    ///
    /// This will perform an initial build and then watch for file changes.
    /// When a file changes, only the affected tasks are re-run.
    ///
    /// # Arguments
    ///
    /// * `data` - The global user data to pass to all tasks.
    #[cfg(feature = "live")]
    pub fn watch(&mut self, data: G) -> anyhow::Result<()> {
        crate::utils::init_logging()?;

        crate::utils::clear_dist()?;
        crate::utils::clone_static()?;

        crate::engine::watch(self, data)?;

        Ok(())
    }
}
