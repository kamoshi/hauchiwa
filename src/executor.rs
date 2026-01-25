mod diagnostics;

use std::{
    collections::{HashMap, HashSet},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, mpsc::Sender},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use camino::Utf8Path;
use crossbeam_channel::unbounded;
use indicatif::ProgressStyle;
use petgraph::graph::NodeIndex;
use petgraph::{algo::toposort, visit::Dfs};
use tracing::{Level, error, info, span};
use tracing_indicatif::span_ext::IndicatifSpanExt;

pub use crate::executor::diagnostics::Diagnostics;
use crate::graph::NodeData;
use crate::{Environment, ImportMap, Mode, Output, Store, TaskContext, Website};

#[cfg(feature = "live")]
pub use live::watch;

#[derive(Debug, Clone)]
pub struct TaskExecution {
    pub start: Instant,
    pub duration: Duration,
}

pub fn run_once_parallel<G: Send + Sync>(
    website: &mut Website<G>,
    globals: &Environment<G>,
) -> anyhow::Result<(HashMap<NodeIndex, NodeData>, Vec<Output>, Diagnostics)> {
    // We run toposort primarily to detect any cycles in the graph.
    toposort(&website.graph, None).expect("Cycle detected in task graph");

    let mut cache = HashMap::new();
    let pending = website.graph.node_indices().collect();

    let diagnostics = run_tasks_parallel(website, globals, &mut cache, &pending)?;

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
fn run_tasks_parallel<G: Send + Sync>(
    site: &Website<G>,
    globals: &Environment<G>,
    cache: &mut HashMap<NodeIndex, NodeData>,
    nodes_to_run: &HashSet<NodeIndex>,
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

    let root_span = span!(Level::INFO, "building_tasks");
    root_span.pb_set_length(total_tasks);
    root_span.pb_set_style(
        &ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    root_span.pb_set_message("Building tasks...");
    let _enter = root_span.enter();

    // We only need a channel for results and tasks are distributed by Rayon.
    let (result_sender, result_receiver) =
        unbounded::<(NodeIndex, anyhow::Result<NodeData>, Instant, Duration)>();

    let mut execution_times = HashMap::new();

    // regular task style with no progress
    let pb_style = crate::utils::get_style_task()?;

    rayon::scope(|s| -> anyhow::Result<()> {
        // A helper closure to spawn a task
        let spawn_task = |cache: &HashMap<NodeIndex, NodeData>, index: NodeIndex| {
            // Prepare dependencies
            let mut dependencies = Vec::new();
            let mut importmap = ImportMap::new();

            for dep_index in site.graph[index].dependencies() {
                let node_data = cache.get(&dep_index).unwrap();
                dependencies.push(node_data.output.clone());
                importmap.merge(node_data.importmap.clone());
            }

            let task = site.graph[index].clone();

            // Clone variables for the thread
            let sender = result_sender.clone();
            let pb_style = pb_style.clone();

            // Spawn on Rayon pool
            s.spawn(move |_| {
                // Tracing span
                let span = span!(Level::INFO, "task", name = task.get_name());
                span.pb_set_style(&pb_style);
                span.pb_set_message(&format!("Running {}", task.get_name()));
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
                    task.execute(&context, &mut rt, &dependencies)
                        .map(|output| {
                            let mut imports = importmap.clone();
                            imports.merge(rt.imports);
                            NodeData {
                                output,
                                importmap: imports,
                            }
                        })
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
                sender.send((index, output, start_time, elapsed)).unwrap();
            });
        };

        // Seed initial tasks
        for &node_index in nodes_to_run {
            if dependency_counts.get(&node_index).cloned().unwrap_or(0) == 0 {
                spawn_task(cache, node_index);
            }
        }

        // Scheduler loop
        // The main thread sits here while Rayon workers execute tasks.
        while completed_tasks < total_tasks {
            // Wait for any task to finish
            let (completed_index, output, start, duration) = result_receiver.recv().unwrap();

            // Update state
            cache.insert(completed_index, output?);
            execution_times.insert(completed_index, TaskExecution { start, duration });
            completed_tasks += 1;
            root_span.pb_inc(1);

            // Unlock dependents
            if let Some(dependents_of_completed) = dependents.get(&completed_index) {
                for &index in dependents_of_completed {
                    if let Some(count) = dependency_counts.get_mut(&index) {
                        *count -= 1;
                        if *count == 0 {
                            // Dependency satisfied, spawn immediately
                            spawn_task(cache, index);
                        }
                    }
                }
            }
        }

        Ok(())
    })?;

    info!("Build complete!");
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
        }
    }
    pages
}

#[cfg(feature = "live")]
mod live {
    //! Watch mode is implemented as a three-part system:
    //!
    //! 1. **File watcher**: Uses the `notify` crate to monitor filesystem
    //!    events recursively. It includes debouncing to prevent duplicate builds
    //!    from rapid file saves.
    //! 2. **WebSocket server**: Spawns a dedicated thread using `tungstenite`
    //!    to maintain persistent connections with open browser tabs.
    //! 3. **Client script**: The [`Environment`](crate::Environment) injects
    //!    a lightweight JavaScript snippet into generated pages. This script
    //!    connects to the WebSocket server and listens for a `"reload"` message.
    //!
    //! ## The Loop
    //!
    //! When a file change is detected:
    //! 1. The graph identifies and rebuilds only the "dirty" subgraph
    //!    (incremental build).
    //! 2. Upon successful completion, the executor signals the WebSocket
    //!    thread.
    //! 3. The server broadcasts the reload command to all connected clients,
    //!    triggering an immediate browser refresh.

    use super::*;

    use std::env;

    use camino::Utf8PathBuf;
    use glob::Pattern;
    use notify::RecursiveMode;
    use notify_debouncer_full::new_debouncer;
    use petgraph::visit::IntoNodeReferences;
    use tungstenite::WebSocket;

    pub fn watch<G: Send + Sync>(site: &mut Website<G>, data: G) -> anyhow::Result<()> {
        let (tcp, port) = reserve_port().unwrap();
        let pwd = env::current_dir().unwrap();

        let globals = Environment {
            generator: "hauchiwa",
            mode: Mode::Watch,
            port: Some(port),
            data,
        };

        info!("running initial build...");
        let (mut cache, pages, _diagnostics) = run_once_parallel(site, &globals)?;
        info!("collected {} pages", pages.len());
        crate::page::save_pages_to_dist(&pages).expect("Failed to save pages");

        info!("initial build completed, now watching for changes...");
        let clients = Arc::new(Mutex::new(vec![]));

        let _thread_i = new_thread_ws_incoming(tcp, clients.clone());
        let (tx_reload, _thread_o) = new_thread_ws_reload(clients.clone());

        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(Duration::from_millis(250), None, tx).unwrap();

        let mut watched = HashSet::new();
        let mut filters = HashSet::new();
        for (_, task) in site.graph.node_references() {
            for path in &task.get_watched() {
                if let Ok((path, pattern)) = resolve_watch_path(path) {
                    watched.insert(path);
                    filters.insert(pattern);
                } else {
                    error!("failed to resolve path: {}", &path);
                };
            }
        }

        // Collapse watched paths to reduce the number of watches
        let watched = collapse_watch_paths(watched);

        for path in watched {
            info!("watching {}", path);
            debouncer.watch(path, RecursiveMode::Recursive)?;
        }

        #[cfg(feature = "server")]
        let _thread_http = server::start();

        loop {
            match rx.recv() {
                Ok(Ok(events)) => {
                    let mut dirty_nodes = HashSet::new();
                    for de in events {
                        for path in &de.event.paths {
                            if !filters.iter().any(|filter| filter.matches_path(path)) {
                                continue;
                            }

                            if let Some(path) = Utf8Path::from_path(path) {
                                let path = path.strip_prefix(&pwd).unwrap();
                                for index in site.graph.node_indices() {
                                    let task = &site.graph[index];
                                    if task.is_dirty(path) {
                                        dirty_nodes.insert(index);
                                    }
                                }
                            }
                        }
                    }

                    if !dirty_nodes.is_empty() {
                        info!("change detected, re-running tasks...");
                        let mut to_rerun = HashSet::new();
                        for start_node in &dirty_nodes {
                            let mut dfs = Dfs::new(&site.graph, *start_node);
                            while let Some(nx) = dfs.next(&site.graph) {
                                to_rerun.insert(nx);
                            }
                        }

                        let _diagnostics =
                            match run_tasks_parallel(site, &globals, &mut cache, &to_rerun) {
                                Ok(res) => res,
                                Err(e) => {
                                    error!("Error running tasks: {}", e);
                                    continue;
                                }
                            };

                        let pages = collect_pages(&cache);
                        info!("collected {} pages", pages.len());
                        crate::page::save_pages_to_dist(&pages).expect("Failed to save pages");
                        tx_reload.send(()).unwrap();
                        info!("rebuild complete, watching for changes...");
                    }
                }
                Ok(Err(e)) => error!("watch error: {:?}", e),
                Err(e) => error!("watch error: {:?}", e),
            }
        }
    }

    fn reserve_port() -> std::io::Result<(TcpListener, u16)> {
        let listener = match TcpListener::bind("127.0.0.1:1337") {
            Ok(sock) => sock,
            Err(_) => TcpListener::bind("127.0.0.1:0")?,
        };

        let addr = listener.local_addr()?;
        let port = addr.port();
        Ok((listener, port))
    }

    fn new_thread_ws_incoming(
        server: TcpListener,
        client: Arc<Mutex<Vec<WebSocket<TcpStream>>>>,
    ) -> JoinHandle<()> {
        std::thread::spawn(move || {
            for stream in server.incoming() {
                let socket = tungstenite::accept(stream.unwrap()).unwrap();
                client.lock().unwrap().push(socket);
            }
        })
    }

    fn new_thread_ws_reload(
        client: Arc<Mutex<Vec<WebSocket<TcpStream>>>>,
    ) -> (Sender<()>, JoinHandle<()>) {
        let (tx, rx) = std::sync::mpsc::channel();

        let thread = std::thread::spawn(move || {
            while rx.recv().is_ok() {
                let mut clients = client.lock().unwrap();
                let mut broken = vec![];

                for (i, socket) in clients.iter_mut().enumerate() {
                    match socket.send("reload".into()) {
                        Ok(_) => {}
                        Err(tungstenite::error::Error::Io(e)) => {
                            if e.kind() == std::io::ErrorKind::BrokenPipe {
                                broken.push(i);
                            }
                        }
                        Err(e) => {
                            error!("Error: {e:?}");
                        }
                    }
                }

                for i in broken.into_iter().rev() {
                    clients.remove(i);
                }

                // Close all but the last 10 connections
                let len = clients.len();
                if len > 10 {
                    for mut socket in clients.drain(0..len - 10) {
                        socket.close(None).ok();
                    }
                }
            }
        });

        (tx, thread)
    }

    /// Splits a glob string into a canonicalized static root path (for
    /// watching) and a compiled absolute Pattern (for matching).
    pub fn resolve_watch_path(glob_str: impl AsRef<str>) -> anyhow::Result<(Utf8PathBuf, Pattern)> {
        let path = Utf8Path::new(glob_str.as_ref());

        // Split path into static root and dynamic suffix (containing wildcards)
        let components: Vec<_> = path.components().collect();
        let split_idx = components
            .iter()
            .position(|c| c.as_str().contains(['*', '?', '[']))
            .unwrap_or(components.len());

        let root_part: Utf8PathBuf = components.iter().take(split_idx).collect();
        let suffix_part: Utf8PathBuf = components.iter().skip(split_idx).collect();

        // Canonicalize the static root (must exist on disk)
        let absolute_root = root_part.canonicalize_utf8()?;

        // If the suffix is empty, we must check if the root is a file or
        // directory. If it's a file, we watch its parent to ensure atomic
        // writes are caught.
        let (watch_root, match_pattern_str) =
            if suffix_part.as_str().is_empty() && absolute_root.is_file() {
                // Case: Concrete File (e.g., "README.md") -> Watch Parent, Match File
                let parent = absolute_root
                    .parent()
                    .unwrap_or(&absolute_root)
                    .to_path_buf();
                (parent, absolute_root)
            } else {
                // Case: Directory (e.g., "src/") or Wildcard (e.g., "src/**/*.rs")
                // -> Watch Dir, Match Pattern
                let pattern_str = absolute_root.join(&suffix_part);
                (absolute_root, pattern_str)
            };

        let pattern = Pattern::new(watch_root.join(match_pattern_str).as_str())?;

        Ok((watch_root, pattern))
    }

    /// Reduces a set of paths to the minimal set of watch roots.
    ///
    /// If we watch `/a` and `/a/b`, we only need to watch `/a` because
    /// the watcher is recursive. This function sorts the paths and filters
    /// out any path that is a subdirectory of a previously accepted path.
    fn collapse_watch_paths(paths: HashSet<Utf8PathBuf>) -> Vec<Utf8PathBuf> {
        let mut paths: Vec<_> = paths.into_iter().collect();
        paths.sort();

        let mut filtered = Vec::new();
        for path in paths {
            if let Some(last) = filtered.last()
                && path.starts_with(last)
            {
                continue;
            }
            filtered.push(path);
        }

        filtered
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_concrete_file() {
            // Input: "README.md" (concrete file)
            let (watch, pattern) = resolve_watch_path("README.md").expect("Should resolve");

            let cwd = Utf8PathBuf::try_from(std::env::current_dir().unwrap()).unwrap();

            // Expectation:
            // Watch: "$CWD/README.md"
            // Pattern: "$CWD/README.md"
            assert_eq!(watch.as_str(), cwd);
            assert_eq!(pattern.as_str(), cwd.join("README.md"));
        }

        #[test]
        fn test_concrete_directory() {
            // Input: "src" (concrete directory)
            let (watch, pattern) = resolve_watch_path("src").expect("Should resolve");

            let cwd = Utf8PathBuf::try_from(std::env::current_dir().unwrap()).unwrap();

            // Expectation:
            // Watch: "src" directory
            // Pattern: "src"
            assert_eq!(watch.as_str(), cwd.join("src"));
            assert_eq!(pattern.as_str(), cwd.join("src"));
        }

        #[test]
        fn test_directory_wildcard() {
            // Input: "src/**/*.rs"
            let (watch, pattern) = resolve_watch_path("src/**/*.rs").expect("Should resolve");

            let cwd = Utf8PathBuf::try_from(std::env::current_dir().unwrap()).unwrap();

            // Expectation:
            // Watch: "src" directory (the static part)
            // Pattern: "src/**/*.rs"
            assert_eq!(watch.as_str(), cwd.join("src/"));
            assert_eq!(pattern.as_str(), cwd.join("src/**/*.rs"));
        }

        #[test]
        fn test_collapse_watch_paths() {
            let mut paths = HashSet::new();
            paths.insert(Utf8PathBuf::from("/a"));
            paths.insert(Utf8PathBuf::from("/a/b"));
            paths.insert(Utf8PathBuf::from("/a/b/c"));
            paths.insert(Utf8PathBuf::from("/b"));
            paths.insert(Utf8PathBuf::from("/c/d"));

            let collapsed = collapse_watch_paths(paths);

            // Expected: /a, /b, /c/d
            // /a/b and /a/b/c are covered by /a.
            assert_eq!(
                collapsed,
                vec![
                    Utf8PathBuf::from("/a"),
                    Utf8PathBuf::from("/b"),
                    Utf8PathBuf::from("/c/d")
                ]
            );
        }

        #[test]
        fn test_collapse_watch_paths_siblings() {
            let mut paths = HashSet::new();
            paths.insert(Utf8PathBuf::from("/a/x"));
            paths.insert(Utf8PathBuf::from("/a/y"));

            let collapsed = collapse_watch_paths(paths);

            // Expected: /a/x, /a/y (neither is a parent of the other)
            assert_eq!(
                collapsed,
                vec![Utf8PathBuf::from("/a/x"), Utf8PathBuf::from("/a/y")]
            );
        }

        #[test]
        fn test_collapse_watch_paths_similar_names() {
            let mut paths = HashSet::new();
            paths.insert(Utf8PathBuf::from("/foo"));
            paths.insert(Utf8PathBuf::from("/foo-bar"));

            let collapsed = collapse_watch_paths(paths);

            // Expected: /foo, /foo-bar
            // /foo-bar is not a subdirectory of /foo
            assert_eq!(
                collapsed,
                vec![Utf8PathBuf::from("/foo"), Utf8PathBuf::from("/foo-bar")]
            );
        }
    }
}

#[cfg(feature = "server")]
mod server {
    use std::{net::SocketAddr, thread};

    use axum::Router;
    use console::style;
    use tower_http::services::ServeDir;
    use tracing::info;

    pub fn start() -> thread::JoinHandle<Result<(), anyhow::Error>> {
        let port = 8080;

        info!(url = %style(format!("http://localhost:{port}/")).yellow(), "starting a HTTP server");

        thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(serve(port))
        })
    }

    async fn serve(port: u16) -> Result<(), anyhow::Error> {
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let address = tokio::net::TcpListener::bind(address).await?;

        let router = Router::new()
            // path to the dist directory with generated website
            .fallback_service(ServeDir::new("dist"));

        axum::serve(address, router).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Dynamic;
    use petgraph::graph::NodeIndex;
    use std::sync::Arc;

    #[test]
    fn test_collect_pages() {
        let mut cache: HashMap<NodeIndex, NodeData> = HashMap::new();
        let page1 = Output {
            url: "/".into(),
            content: "Home".to_string(),
        };
        let page2 = Output {
            url: "/about".into(),
            content: "About".to_string(),
        };
        let page3 = Output {
            url: "/contact".into(),
            content: "Contact".to_string(),
        };

        cache.insert(
            NodeIndex::new(0),
            NodeData {
                output: Arc::new(page1.clone()) as Dynamic,
                importmap: ImportMap::default(),
            },
        );
        cache.insert(
            NodeIndex::new(1),
            NodeData {
                output: Arc::new(vec![page2.clone(), page3.clone()]) as Dynamic,
                importmap: ImportMap::default(),
            },
        );
        cache.insert(
            NodeIndex::new(2),
            NodeData {
                output: Arc::new("not a page".to_string()) as Dynamic,
                importmap: ImportMap::default(),
            },
        );

        let pages = collect_pages(&cache);

        assert_eq!(pages.len(), 3);
        assert!(pages.iter().any(|p| p.url == "/"));
        assert!(pages.iter().any(|p| p.url == "/about"));
        assert!(pages.iter().any(|p| p.url == "/contact"));
    }
}
