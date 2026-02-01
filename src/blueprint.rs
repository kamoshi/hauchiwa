use std::any::type_name;
use std::borrow::Cow;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::sync::Arc;

use petgraph::Graph;
use petgraph::graph::NodeIndex;

use crate::TaskContext;
use crate::core::{Dynamic, Environment, Mode};
use crate::engine::{
    Dependencies, HandleC, HandleF, Task, TrackerState, Tracking, TypedCoarse, TypedFine,
};
use crate::loader::Store;

/// The blueprint for your static site.
///
/// `Blueprint` is used to define the Task graph of your website. You add tasks
/// (including loaders) to the config, and wire them together using their
/// [`Handle`]s.
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
        Self {
            graph: Graph::new(),
        }
    }

    pub fn finish(self) -> Website<G> {
        Website { graph: self.graph }
    }

    /// The entry point. Starts in the "Empty" state.
    pub fn task(&mut self) -> TaskDef<'_, G> {
        TaskDef {
            blueprint: self,
            name: None,
        }
    }

    pub(crate) fn add_task_fine<O, T>(&mut self, task: T) -> HandleF<O>
    where
        O: 'static,
        T: TypedFine<G, Output = O> + 'static,
    {
        let dependencies = task.dependencies();
        let index = self.graph.add_node(Task::F(Arc::new(task)));

        for dependency in dependencies {
            self.graph.add_edge(dependency, index, ());
        }

        HandleF::new(index)
    }

    pub(crate) fn add_task_coarse<O, T>(&mut self, task: T) -> HandleC<O>
    where
        O: 'static,
        T: TypedCoarse<G, Output = O> + 'static,
    {
        let dependencies = task.dependencies();
        let index = self.graph.add_node(Task::C(Arc::new(task)));

        for dependency in dependencies {
            self.graph.add_edge(dependency, index, ());
        }

        HandleC::new(index)
    }
}

impl<G: Send + Sync + 'static> Default for Blueprint<G> {
    fn default() -> Self {
        Self::new()
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

    pub fn source(self, glob: impl Into<String>) -> TaskSourceBinder<'a, G> {
        TaskSourceBinder {
            blueprint: self.blueprint,
            name: self.name,
            sources: vec![glob.into()],
        }
    }

    pub fn depends_on<D>(self, dependencies: D) -> TaskBinder<'a, G, D>
    where
        D: Dependencies,
    {
        TaskBinder {
            blueprint: self.blueprint,
            name: self.name,
            dependencies,
        }
    }

    pub fn run<F, R>(self, callback: F) -> HandleC<R>
    where
        F: Fn(&TaskContext<'_, G>) -> anyhow::Result<R> + Send + Sync + 'static,
        R: Send + Sync + 'static,
    {
        self.blueprint.add_task_coarse(TaskNode {
            name: self.name.unwrap_or(type_name::<F>().into()),
            dependencies: (),
            callback: move |ctx, _| callback(ctx),
            _phantom: PhantomData,
        })
    }
}

pub struct TaskSourceBinder<'a, G: Send + Sync> {
    blueprint: &'a mut Blueprint<G>,
    name: Option<Cow<'static, str>>,
    sources: Vec<String>,
}

impl<'a, G: Send + Sync + 'static> TaskSourceBinder<'a, G> {
    pub fn name(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn source(mut self, glob: impl Into<String>) -> Self {
        self.sources.push(glob.into());
        self
    }

    pub fn run<F, R>(self, callback: F) -> Result<HandleF<R>, crate::error::HauchiwaError>
    where
        F: Fn(&TaskContext<G>, &mut Store, crate::loader::Input) -> anyhow::Result<R>
            + Send
            + Sync
            + 'static,
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

    pub fn run<F, R>(self, callback: F) -> HandleC<R>
    where
        F: for<'b> Fn(&TaskContext<'b, G>, D::Output<'b>) -> anyhow::Result<R>
            + Send
            + Sync
            + 'static,
        R: Send + Sync + 'static,
    {
        self.blueprint.add_task_coarse(TaskNode {
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
    /// 4. Save the generated [`Output`]s to `dist`.
    ///
    /// # Arguments
    ///
    /// * `data` - The global user data to pass to all tasks.
    pub fn build(&mut self, data: G) -> anyhow::Result<crate::executor::Diagnostics> {
        crate::utils::init_logging()?;

        let globals = Environment {
            generator: "hauchiwa",
            mode: Mode::Build,
            port: None,
            data,
        };

        crate::utils::clear_dist()?;
        crate::utils::clone_static()?;

        let (_, pages, diagnostics) = crate::executor::run_once_parallel(self, &globals)?;

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

        crate::executor::watch(self, data)?;

        Ok(())
    }
}

pub(crate) struct TaskNode<G, R, D, F>
where
    G: Send + Sync,
    R: Send + Sync + 'static,
    D: Dependencies,
    F: for<'a> Fn(&TaskContext<'a, G>, D::Output<'a>) -> anyhow::Result<R> + Send + Sync,
{
    pub name: Cow<'static, str>,
    pub dependencies: D,
    pub callback: F,
    pub _phantom: PhantomData<G>,
}

impl<G, R, D, F> TypedCoarse<G> for TaskNode<G, R, D, F>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
    D: Dependencies + Send + Sync,
    F: for<'a> Fn(&TaskContext<'a, G>, D::Output<'a>) -> anyhow::Result<R> + Send + Sync + 'static,
{
    type Output = R;

    fn get_name(&self) -> String {
        self.name.to_string()
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        self.dependencies.dependencies()
    }

    fn get_watched(&self) -> Vec<camino::Utf8PathBuf> {
        vec![]
    }

    fn execute(
        &self,
        context: &TaskContext<G>,
        _: &mut Store,
        dependencies: &[Dynamic],
    ) -> anyhow::Result<(Tracking, Self::Output)> {
        let (tracking, dependencies) = self.dependencies.resolve(dependencies);
        let output = (self.callback)(context, dependencies)?;
        Ok((tracking, output))
    }

    fn is_valid(
        &self,
        old_tracking: &[Option<TrackerState>],
        new_outputs: &[Dynamic],
        updated_nodes: &HashSet<NodeIndex>,
    ) -> bool {
        self.dependencies
            .is_valid(old_tracking, new_outputs, updated_nodes)
    }
}
