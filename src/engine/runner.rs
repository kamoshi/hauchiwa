mod diagnostics;
#[cfg(feature = "server")]
mod http;
#[cfg(feature = "live")]
mod watch;

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use anyhow::Context;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use petgraph::Graph;
use petgraph::graph::NodeIndex;
use tracing::Level;
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::core::{Dynamic, Store};
use crate::engine::{Map, Task, TrackerState};
use crate::error::{BuildError, HauchiwaError};
use crate::snapshot::Snapshot;
use crate::{Environment, ImportMap, Output, TaskContext, Website};

#[cfg(feature = "live")]
pub(crate) use watch::watch;

pub use diagnostics::Diagnostics;

#[derive(Debug, Clone)]
pub struct TaskTiming {
    pub start: Instant,
    pub duration: Duration,
}

/// Represents the data stored in the graph for each node.
/// Includes the user's output, the concatenated import map, and any
/// content-addressed assets saved to `dist/hash/` via [`Store::save`].
#[derive(Clone, Debug)]
pub(crate) struct NodeData {
    pub output: Dynamic,
    pub tracking: Vec<Option<TrackerState>>,
    pub importmap: ImportMap,
    /// Dist-relative paths of hash assets produced by this node (e.g. `hash/abc123.png`).
    /// Retained across cache hits so the Snapshot always has a complete picture.
    pub store_paths: Vec<Utf8PathBuf>,
}

#[derive(Debug, thiserror::Error)]
enum SchedulerError {
    #[error("Scheduler state mutex was poisoned")]
    Poisoned,
    #[error("Task {node:?} is missing dependency {dependency:?}")]
    MissingDependency {
        node: NodeIndex,
        dependency: NodeIndex,
    },
    #[error("Dependency count underflow for task {node:?}")]
    DependencyCountUnderflow { node: NodeIndex },
    #[error("Some selected tasks never completed ({completed}/{selected})")]
    Incomplete { completed: usize, selected: usize },
    #[error("Scheduler panicked: {message}")]
    Panicked { message: String },
}

#[derive(Debug, thiserror::Error)]
#[error("Task panicked: {message}")]
struct TaskPanic {
    message: String,
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("unknown payload")
        .to_owned()
}

/// Poisoned bookkeeping must never be used to release more tasks.
fn lock_state(
    state: &Mutex<SchedulerState>,
) -> Result<MutexGuard<'_, SchedulerState>, SchedulerError> {
    state.lock().map_err(|_| SchedulerError::Poisoned)
}

struct SchedulerState {
    // The most recently available result for each task.
    cache: HashMap<NodeIndex, NodeData>,
    // For each task selected for this run, the number of prerequisites that
    // still need to complete during this run.
    remaining_dependencies: HashMap<NodeIndex, usize>,
    // The tasks whose executors actually ran during this invocation.
    updated_nodes: HashSet<NodeIndex>,
    // Timing information for diagnostics.
    execution_times: HashMap<NodeIndex, TaskTiming>,
    // The number of selected tasks that have successfully completed, including
    // whole-node cache hits.
    completed: usize,
    // The first task error observed by the scheduler.
    first_error: Option<anyhow::Error>,
}

impl SchedulerState {
    fn new(
        cache: HashMap<NodeIndex, NodeData>,
        remaining_dependencies: HashMap<NodeIndex, usize>,
    ) -> Self {
        Self {
            cache,
            remaining_dependencies,
            updated_nodes: HashSet::new(),
            execution_times: HashMap::new(),
            completed: 0,
            first_error: None,
        }
    }

    /// Return cached results even on failure, so watch mode retains completed work.
    fn finish(self, cache: &mut HashMap<NodeIndex, NodeData>) -> anyhow::Result<Diagnostics> {
        *cache = self.cache;

        if let Some(error) = self.first_error {
            return Err(error);
        }

        if self.completed != self.remaining_dependencies.len() {
            return Err(SchedulerError::Incomplete {
                completed: self.completed,
                selected: self.remaining_dependencies.len(),
            }
            .into());
        }

        Ok(Diagnostics {
            execution_times: self.execution_times,
        })
    }

    fn find_roots(&self) -> Vec<NodeIndex> {
        self.remaining_dependencies
            .iter()
            .filter_map(|(&node, &count)| (count == 0).then_some(node))
            .collect()
    }
}

// Count dependencies for each node that we intend to run.
// A dependency only counts if it's also in the set of nodes to run.
fn count_dependencies<G: Send + Sync>(
    website: &Website<G>,
    nodes_to_run: &HashSet<NodeIndex>,
) -> HashMap<NodeIndex, usize> {
    nodes_to_run
        .iter()
        .map(|&i| {
            (
                i,
                website
                    .graph
                    .neighbors_directed(i, petgraph::Direction::Incoming)
                    .filter(|dep| nodes_to_run.contains(dep))
                    .count(),
            )
        })
        .collect()
}

struct PreparedTask {
    dependencies: Vec<Dynamic>,
    dependency_imports: Vec<ImportMap>,
    previous_data: Option<NodeData>,
    updated_nodes: HashSet<NodeIndex>,
}

impl PreparedTask {
    fn can_reuse<G: Send + Sync>(&self, task: &Task<G>) -> bool {
        self.previous_data.as_ref().is_some_and(|previous| {
            task.is_still_valid(&previous.tracking, &self.dependencies, &self.updated_nodes)
        })
    }
}

struct CompletedTask {
    data: NodeData,
    executed: bool,
    timing: TaskTiming,
}

impl CompletedTask {
    fn reused(data: NodeData, start: Instant) -> Self {
        Self {
            data,
            executed: false,
            timing: TaskTiming {
                start,
                duration: Duration::ZERO,
            },
        }
    }

    fn executed(data: NodeData, timing: TaskTiming) -> Self {
        Self {
            data,
            executed: true,
            timing,
        }
    }
}

impl SchedulerState {
    /// Capture inputs while the caller holds the scheduler lock.
    fn prepare_node<G: Send + Sync>(
        &mut self,
        website: &Website<G>,
        node: NodeIndex,
    ) -> Option<PreparedTask> {
        if self.first_error.is_some() {
            return None;
        }

        let mut dependencies = Vec::new();
        let mut dependency_imports = Vec::new();
        for dependency in website.graph[node].dependencies() {
            let Some(data) = self.cache.get(&dependency) else {
                self.first_error =
                    Some(SchedulerError::MissingDependency { node, dependency }.into());

                return None;
            };

            dependencies.push(data.output.clone());
            dependency_imports.push(data.importmap.clone());
        }

        Some(PreparedTask {
            dependencies,
            dependency_imports,
            previous_data: self.cache.get(&node).cloned(),
            updated_nodes: self.updated_nodes.clone(),
        })
    }

    /// Record completion under the lock. None means no further work should be
    /// released. An empty Vec means completion succeeded without ready
    /// dependents.
    fn complete_node<G: Send + Sync>(
        &mut self,
        website: &Website<G>,
        node: NodeIndex,
        completion: CompletedTask,
    ) -> Option<Vec<NodeIndex>> {
        self.cache.insert(node, completion.data);
        self.execution_times.insert(node, completion.timing);
        self.completed += 1;

        if completion.executed {
            self.updated_nodes.insert(node);
        }

        // Preserve successful results from jobs already running when another failed.
        if self.first_error.is_some() {
            return None;
        }

        let mut ready = Vec::new();
        for downstream_node in website
            .graph
            .neighbors_directed(node, petgraph::Direction::Outgoing)
        {
            // Missing entries are dependents outside this incremental run.
            let Some(count) = self.remaining_dependencies.get_mut(&downstream_node) else {
                continue;
            };

            let Some(remaining) = count.checked_sub(1) else {
                self.first_error = Some(
                    SchedulerError::DependencyCountUnderflow {
                        node: downstream_node,
                    }
                    .into(),
                );

                return None;
            };

            *count = remaining;

            if remaining == 0 {
                ready.push(downstream_node);
            }
        }

        Some(ready)
    }
}

/// Validate or execute using captured inputs, without accessing the scheduler mutex.
fn execute_or_reuse<G: Send + Sync>(
    website: &Website<G>,
    globals: &Environment<G>,
    node: NodeIndex,
    is_marked_dirty: bool,
    prepared: PreparedTask,
) -> anyhow::Result<CompletedTask> {
    let start = Instant::now();

    // Catch task panics without unwinding through the scheduler.
    // No scheduler lock is held, and failed results are not published.
    // Callback-owned shared state and filesystem writes are not rolled back.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let task = &website.graph[node];

        if !is_marked_dirty
            && prepared.can_reuse(task)
            && let Some(previous) = prepared.previous_data.as_ref()
        {
            return Ok(CompletedTask::reused(previous.clone(), start));
        }

        let mut importmap = ImportMap::new();
        for imports in prepared.dependency_imports {
            importmap.merge(imports);
        }

        let span = tracing::span!(Level::INFO, "task", name = task.name());
        span.pb_set_style(&website.progress.task);
        span.pb_set_message(&format!("Running {}", task.name()));
        let _enter = span.enter();

        let context = TaskContext {
            env: globals,
            importmap: &importmap,
            span: span.clone(),
            progress: &website.progress,
        };

        let mut store = Store::with_dirs(website.out_dir.clone(), website.cache_dir.clone());

        // Preserve the old runner's behavior for directly invalidated nodes.
        let old_output = prepared
            .previous_data
            .as_ref()
            .filter(|_| !is_marked_dirty)
            .map(|data| &data.output);

        let (tracking, output) = match task {
            Task::C(task) => task.execute(&context, &mut store, &prepared.dependencies)?,
            Task::F(task) => task.execute(
                &context,
                &mut store,
                &prepared.dependencies,
                old_output,
                &prepared.updated_nodes,
            )?,
        };

        let tracking = tracking.into_states()?;
        importmap.merge(store.imports);

        Ok::<_, anyhow::Error>(CompletedTask::executed(
            NodeData {
                output,
                tracking,
                importmap,
                store_paths: store.store_paths,
            },
            TaskTiming {
                start,
                duration: start.elapsed(),
            },
        ))
    }));

    result
        .map_err(|panic| {
            anyhow::Error::new(TaskPanic {
                message: panic_message(panic.as_ref()),
            })
        })
        .and_then(|result| result)
        .with_context(|| format!("Task {:?} ({node:?}) failed", website.graph[node].name()))
}

fn spawn_node<'scope, G: Send + Sync>(
    scope: &rayon::Scope<'scope>,
    website: &'scope Website<G>,
    globals: &'scope Environment<G>,
    state: &'scope Mutex<SchedulerState>,
    node: NodeIndex,
    dirty: &'scope HashSet<NodeIndex>,
) {
    let parent_span = tracing::Span::current();

    scope.spawn(move |scope| {
        let _enter = parent_span.enter();

        let prepared = {
            let Ok(mut guard) = lock_state(state) else {
                // The boundary reports poisoning after all scoped jobs finish.
                return;
            };
            guard.prepare_node(website, node)
        };

        let Some(prepared) = prepared else {
            return;
        };

        let completion =
            match execute_or_reuse(website, globals, node, dirty.contains(&node), prepared) {
                Ok(completion) => completion,
                Err(error) => {
                    let Ok(mut guard) = lock_state(state) else {
                        // The boundary reports poisoning after all scoped jobs finish.
                        return;
                    };
                    guard.first_error.get_or_insert(error);
                    return;
                }
            };

        let executed = completion.executed;
        let duration = completion.timing.duration;
        let newly_ready = {
            let Ok(mut guard) = lock_state(state) else {
                // The boundary reports poisoning after all scoped jobs finish.
                return;
            };
            guard.complete_node(website, node, completion)
        };
        let Some(newly_ready) = newly_ready else {
            return;
        };

        parent_span.pb_inc(1);
        if executed {
            tracing::info!(
                target: "task",
                name = website.graph[node].name(),
                duration_ms = duration.as_millis() as u64,
                "Finished task"
            );
        }

        for next in newly_ready {
            spawn_node(scope, website, globals, state, next, dirty);
        }
    });
}

fn run_inner<G: Send + Sync>(
    website: &Website<G>,
    globals: &Environment<G>,
    state: &Mutex<SchedulerState>,
    dirty: &HashSet<NodeIndex>,
) -> Result<(), SchedulerError> {
    let roots = lock_state(state)?.find_roots();

    let parent_span = tracing::Span::current();
    rayon::scope(|scope| {
        let _enter = parent_span.enter();
        for node in roots {
            spawn_node(scope, website, globals, state, node, dirty);
        }
    });

    Ok(())
}

/// The initial graph result, before static files are added or outputs committed.
pub(crate) struct InitialRun {
    pub cache: HashMap<NodeIndex, NodeData>,
    pub snapshot: Snapshot,
    pub diagnostics: Diagnostics,
}

/// Validate and execute the entire graph with a fresh task cache.
pub(crate) fn run_initial<G: Send + Sync>(
    website: &Website<G>,
    globals: &Environment<G>,
) -> Result<InitialRun, HauchiwaError> {
    petgraph::algo::toposort(&website.graph, None).map_err(|_| HauchiwaError::GraphCycle)?;

    let mut cache = HashMap::new();
    let selected = website.graph.node_indices().collect();
    let dirty = HashSet::new();

    let diagnostics =
        run_selected(website, globals, &mut cache, &selected, &dirty).map_err(BuildError::Other)?;
    let snapshot = collect_manifest(&cache, &website.graph)?;

    Ok(InitialRun {
        cache,
        snapshot,
        diagnostics,
    })
}

/// Executes selected tasks using completion-driven scheduling. Each completed
/// task releases its ready dependents into the same Rayon scope. Dependencies
/// outside the selected set are resolved from the retained cache.
pub(crate) fn run_selected<G: Send + Sync>(
    site: &Website<G>,
    globals: &Environment<G>,
    cache: &mut HashMap<NodeIndex, NodeData>,
    nodes_to_run: &HashSet<NodeIndex>,
    explicitly_dirty: &HashSet<NodeIndex>,
) -> anyhow::Result<Diagnostics> {
    if nodes_to_run.is_empty() {
        return Ok(Diagnostics::default());
    }

    let root_span = tracing::span!(Level::INFO, "building_tasks");
    root_span.pb_set_length(nodes_to_run.len() as u64);
    root_span.pb_set_style(&site.progress.build);
    root_span.pb_set_message("Building tasks...");
    let _enter = root_span.enter();

    let remaining_dependencies = count_dependencies(site, nodes_to_run);
    let state = Mutex::new(SchedulerState::new(
        std::mem::take(cache),
        remaining_dependencies,
    ));

    // A bookkeeping panic is different from a user-task failure. Rayon waits
    // for scoped jobs before propagating it. Catch it here so the caller gets
    // an error and the borrowed cache is not accidentally lost during unwinding.
    let execution = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_inner(site, globals, &state, explicitly_dirty)
    }));

    let execution = execution
        .map_err(|panic| SchedulerError::Panicked {
            message: panic_message(panic.as_ref()),
        })
        .and_then(|result| result);

    // Do not recover and reuse potentially inconsistent state after poisoning.
    // The caller's cache remains empty and watch mode treats this as fatal.
    let state = state.into_inner().map_err(|_| SchedulerError::Poisoned)?;

    if let Err(error) = execution {
        *cache = state.cache;
        return Err(error.into());
    }

    let diagnostics = state.finish(cache)?;

    tracing::info!("Build complete!");
    Ok(diagnostics)
}

pub(crate) fn collect_manifest<G: Send + Sync>(
    cache: &HashMap<NodeIndex, NodeData>,
    graph: &Graph<Task<G>, ()>,
) -> Result<Snapshot, crate::error::BuildError> {
    let mut manifest = Snapshot::new();
    for (index, node_data) in cache {
        let task_name = graph[*index].name();
        let value = &node_data.output;

        if let Some(page) = value.downcast_ref::<Output>() {
            manifest.insert_page(*index, &task_name, page.clone())?;
        } else if let Some(page_vec) = value.downcast_ref::<Vec<Output>>() {
            for page in page_vec {
                manifest.insert_page(*index, &task_name, page.clone())?;
            }
        } else if let Some(page_map) = value.downcast_ref::<Map<Output>>() {
            for (item, _) in page_map.map.values() {
                manifest.insert_page(*index, &task_name, item.clone())?;
            }
        }

        for path in &node_data.store_paths {
            manifest.insert_hash_asset(*index, &task_name, path.clone())?;
        }
    }
    Ok(manifest)
}
