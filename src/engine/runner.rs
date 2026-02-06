mod diagnostics;
#[cfg(feature = "server")]
mod http;
#[cfg(feature = "live")]
mod watch;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use indicatif::ProgressStyle;
use petgraph::graph::NodeIndex;
use tracing::Level;
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::core::{Dynamic, Store};
use crate::engine::{Map, Task, TrackerState};
use crate::{Environment, ImportMap, Output, TaskContext, Website};

#[cfg(feature = "live")]
pub(crate) use watch::watch;

pub use diagnostics::Diagnostics;

#[derive(Debug, Clone)]
pub struct TaskExecution {
    pub start: Instant,
    pub duration: Duration,
}

/// Represents the data stored in the graph for each node.
/// Includes the user's output and the concatenated import map.
#[derive(Clone, Debug)]
pub(crate) struct NodeData {
    pub output: Dynamic,
    pub tracking: Vec<Option<TrackerState>>,
    pub importmap: ImportMap,
}

pub(crate) fn run_once_parallel<G: Send + Sync>(
    website: &mut Website<G>,
    globals: &Environment<G>,
) -> anyhow::Result<(HashMap<NodeIndex, NodeData>, Vec<Output>, Diagnostics)> {
    // We run toposort primarily to detect any cycles in the graph.
    petgraph::algo::toposort(&website.graph, None).expect("Cycle detected in task graph");

    let mut cache = HashMap::new();
    let pending = website.graph.node_indices().collect();
    let dirty = HashSet::new();

    let diagnostics = run_tasks_parallel(website, globals, &mut cache, &pending, &dirty)?;

    let pages = collect_pages(&cache);
    Ok((cache, pages, diagnostics))
}

/// This function executes the task graph using a thread pool. It performs a
/// parallel topological sort of the graph, where tasks are executed as soon as
/// their dependencies are met.
///
/// The algorithm works as follows:
/// 1. A pool of worker threads is spawned.
/// 2. Two channels are created: one for sending tasks to the workers and one
///    for receiving results back.
/// 3. The initial set of tasks (those with no dependencies) is sent to the
///    workers.
/// 4. The main thread enters a loop, waiting for results from the workers.
/// 5. When a task completes, its result is cached. The dependency counts of
///    all tasks that depend on the completed task are decremented.
/// 6. If a task's dependency count reaches zero, it is sent to the workers.
/// 7. The loop continues until all tasks have been completed.
pub(crate) fn run_tasks_parallel<G: Send + Sync>(
    site: &Website<G>,
    globals: &Environment<G>,
    cache: &mut HashMap<NodeIndex, NodeData>,
    nodes_to_run: &HashSet<NodeIndex>,
    explicitly_dirty: &HashSet<NodeIndex>,
) -> anyhow::Result<Diagnostics> {
    // Build a map from a dependency to the nodes that depend on it for the entire graph.
    let mut dependents: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
    for edge in site.graph.raw_edges() {
        dependents
            .entry(edge.source())
            .or_default()
            .push(edge.target());
    }

    // Count dependencies for each node that we intend to run.
    // A dependency only counts if it's also in the set of nodes to run.
    let mut dependency_counts: HashMap<NodeIndex, usize> = nodes_to_run
        .iter()
        .map(|&i| {
            (
                i,
                site.graph
                    .neighbors_directed(i, petgraph::Direction::Incoming)
                    .filter(|dep| nodes_to_run.contains(dep))
                    .count(),
            )
        })
        .collect();

    let total_tasks = nodes_to_run.len() as u64;
    let mut completed_tasks = 0;

    if total_tasks == 0 {
        return Ok(Diagnostics::default());
    }

    let root_span = tracing::span!(Level::INFO, "building_tasks");
    root_span.pb_set_length(total_tasks);
    root_span.pb_set_style(
        &ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    root_span.pb_set_message("Building tasks...");
    let _enter = root_span.enter();

    let mut execution_times = HashMap::new();
    let mut updated_nodes = HashSet::new();

    // regular task style with no progress
    let pb_style = crate::utils::get_style_task()?;

    rayon::scope(|s| -> anyhow::Result<()> {
        // We only need a channel for results and tasks are distributed by Rayon.
        // (index, result, start, duration, ran_was_executed)
        let (result_sender, result_receiver) =
            channel::<(NodeIndex, anyhow::Result<NodeData>, Instant, Duration, bool)>();

        // A helper closure to spawn a task
        let spawn_task = |cache: &HashMap<NodeIndex, NodeData>,
                          index: NodeIndex,
                          updated_nodes: &HashSet<NodeIndex>| {
            // Prepare dependencies
            let mut dependencies = Vec::new();
            let mut importmap = ImportMap::new();

            for dep_index in site.graph[index].dependencies() {
                let node_data = cache.get(&dep_index).unwrap();
                dependencies.push(node_data.output.clone());
                importmap.merge(node_data.importmap.clone());
            }

            // Check if we can skip this task
            let is_explicitly_dirty = explicitly_dirty.contains(&index);
            let mut should_run = true;
            let mut old_data = None;

            if !is_explicitly_dirty && let Some(data) = cache.get(&index) {
                old_data = Some(data.clone());
                let task = &site.graph[index];
                if task.is_valid(&data.tracking, &dependencies, updated_nodes) {
                    should_run = false;
                }
            }

            if !should_run {
                // Task is skipped
                let sender = result_sender.clone();
                let output = old_data.unwrap();
                sender
                    .send((index, Ok(output), Instant::now(), Duration::ZERO, false))
                    .unwrap();
                return;
            }

            let task = site.graph[index].clone();

            // Clone variables for the thread
            let sender = result_sender.clone();
            let pb_style = pb_style.clone();

            let old_output = old_data.map(|d| d.output);
            let updated_nodes = updated_nodes.clone();

            // Spawn on Rayon pool
            s.spawn(move |_| {
                // Tracing span
                let span = tracing::span!(Level::INFO, "task", name = task.name());
                span.pb_set_style(&pb_style);
                span.pb_set_message(&format!("Running {}", task.name()));
                let _enter = span.enter();

                let context = TaskContext {
                    env: globals,
                    importmap: &importmap,
                    span: span.clone(),
                };

                let start_time = Instant::now();

                // We use AssertUnwindSafe because we are confident that if the
                // specific task logic panics, it won't corrupt the shared
                // memory in a way that affects other threads (since we are
                // using mostly cloned and/or immutable data).
                let output = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut rt = Store::new();

                    match task {
                        Task::C(task) => task.execute(&context, &mut rt, &dependencies).map(
                            |(tracking, output)| {
                                let tracking = tracking.unwrap();
                                let mut imports = importmap.clone();
                                imports.merge(rt.imports);
                                NodeData {
                                    output,
                                    tracking,
                                    importmap: imports,
                                }
                            },
                        ),
                        Task::F(task) => {
                            task.execute(
                                &context,
                                &mut rt,
                                &dependencies,
                                old_output.as_ref(),
                                &updated_nodes,
                            )
                            .map(|(tracking, output)| {
                                let tracking = tracking.unwrap();
                                let mut imports = importmap.clone();
                                imports.merge(rt.imports);
                                NodeData {
                                    output,
                                    tracking,
                                    importmap: imports,
                                }
                            })
                        }
                    }
                })) {
                    Ok(result) => result,
                    Err(panic) => {
                        let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                            format!("Task panicked: {s}")
                        } else if let Some(s) = panic.downcast_ref::<String>() {
                            format!("Task panicked: {s}")
                        } else {
                            String::from("Task panicked with unknown payload")
                        };

                        Err(anyhow::anyhow!(msg))
                    }
                };

                let elapsed = start_time.elapsed();

                // Send result back to main thread
                sender
                    .send((index, output, start_time, elapsed, true))
                    .unwrap();
            });
        };

        // Seed initial tasks
        for &node_index in nodes_to_run {
            if dependency_counts.get(&node_index).cloned().unwrap_or(0) == 0 {
                spawn_task(cache, node_index, &updated_nodes);
            }
        }

        // Scheduler loop
        // The main thread sits here while Rayon workers execute tasks.
        while completed_tasks < total_tasks {
            // Wait for any task to finish
            let (completed_index, output, start, duration, executed) =
                result_receiver.recv().unwrap();

            // Update state
            cache.insert(completed_index, output?);
            execution_times.insert(completed_index, TaskExecution { start, duration });
            completed_tasks += 1;
            root_span.pb_inc(1);

            if executed {
                updated_nodes.insert(completed_index);
            }

            // Unlock dependents
            if let Some(dependents_of_completed) = dependents.get(&completed_index) {
                for &index in dependents_of_completed {
                    if let Some(count) = dependency_counts.get_mut(&index) {
                        *count -= 1;
                        if *count == 0 {
                            // Dependency satisfied, spawn immediately
                            spawn_task(cache, index, &updated_nodes);
                        }
                    }
                }
            }
        }

        Ok(())
    })?;

    tracing::info!("Build complete!");
    Ok(Diagnostics { execution_times })
}

fn collect_pages(cache: &HashMap<NodeIndex, NodeData>) -> Vec<Output> {
    let mut pages: Vec<Output> = Vec::new();
    for node_data in cache.values() {
        let value = &node_data.output;
        if let Some(page) = value.downcast_ref::<Output>() {
            pages.push(page.clone());
        } else if let Some(page_vec) = value.downcast_ref::<Vec<Output>>() {
            pages.extend(page_vec.clone());
        } else if let Some(page_map) = value.downcast_ref::<Map<Output>>() {
            pages.extend(page_map.map.values().map(|(item, _)| item).cloned());
        }
    }
    pages
}
