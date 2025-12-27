use std::{
    collections::{HashMap, HashSet},
    env,
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, mpsc::Sender},
    thread::JoinHandle,
    time::Duration,
};

use camino::Utf8Path;
use crossbeam_channel::unbounded;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use notify::RecursiveMode;
use notify_debouncer_full::new_debouncer;
use petgraph::graph::NodeIndex;
use petgraph::{algo::toposort, visit::Dfs};
use tungstenite::WebSocket;

use crate::{
    Environment, Mode, TaskContext, Website, graph::NodeData, importmap::ImportMap, loader::Store,
    page::Output,
};

pub fn run_once_parallel<G: Send + Sync>(
    site: &mut Website<G>,
    globals: &Environment<G>,
) -> anyhow::Result<(HashMap<NodeIndex, NodeData>, Vec<Output>)> {
    // We run toposort primarily to detect any cycles in the graph.
    toposort(&site.graph, None).expect("Cycle detected in task graph");

    let mut cache: HashMap<NodeIndex, NodeData> = HashMap::new();
    let nodes_to_run: HashSet<NodeIndex> = site.graph.node_indices().collect();

    run_tasks_parallel(site, globals, &mut cache, &nodes_to_run)?;

    let pages = collect_pages(&cache);
    Ok((cache, pages))
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
) -> anyhow::Result<()> {
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
        return Ok(());
    }

    // Setup MultiProgress and the main overall progress bar
    let mp = MultiProgress::new();
    let main_pb = mp.add(ProgressBar::new(total_tasks));
    main_pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    main_pb.set_message("Building tasks...");

    // Define the style for the per-task spinners
    let spinner_style = ProgressStyle::default_spinner()
        .template("{spinner:.blue} {msg}")
        .unwrap();

    // We only need a channel for results and tasks are distributed by Rayon.
    let (result_sender, result_receiver) = unbounded::<(NodeIndex, anyhow::Result<NodeData>)>();

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
            let mp_clone = mp.clone();
            let style_clone = spinner_style.clone();

            // Spawn on Rayon pool
            s.spawn(move |_| {
                let task_pb = mp_clone.add(ProgressBar::new_spinner());
                task_pb.set_style(style_clone);
                task_pb.set_message(task.get_name());
                task_pb.enable_steady_tick(Duration::from_millis(100));

                let context = TaskContext {
                    env: globals,
                    importmap: &importmap,
                };

                let output = {
                    let mut rt = Store::new();

                    task.execute(&context, &mut rt, &dependencies)
                        .map(|output| NodeData {
                            output,
                            importmap: rt.imports,
                        })
                };

                task_pb.finish_and_clear();

                // Send result back to main thread
                sender.send((index, output)).unwrap();
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
            let (completed_index, output) = result_receiver.recv().unwrap();

            // Update state
            cache.insert(completed_index, output?);
            completed_tasks += 1;
            main_pb.inc(1);

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

    main_pb.finish_with_message("Build complete!");
    Ok(())
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

pub fn watch<G: Send + Sync>(site: &mut Website<G>, data: G) -> anyhow::Result<()> {
    let (tcp, port) = reserve_port().unwrap();
    let pwd = env::current_dir().unwrap();

    let globals = Environment {
        generator: "hauchiwa",
        mode: Mode::Watch,
        port: Some(port),
        data,
    };

    println!("Performing initial build...");
    let (mut cache, pages) = run_once_parallel(site, &globals)?;
    println!("Collected {} pages", pages.len());
    crate::page::save_pages_to_dist(&pages).expect("Failed to save pages");

    println!("Initial build complete. Watching for changes...");
    let clients = Arc::new(Mutex::new(vec![]));

    let _thread_i = new_thread_ws_incoming(tcp, clients.clone());
    let (tx_reload, _thread_o) = new_thread_ws_reload(clients.clone());

    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = new_debouncer(Duration::from_millis(250), None, tx).unwrap();
    debouncer
        .watch(Utf8Path::new(".").as_std_path(), RecursiveMode::Recursive)
        .unwrap();

    #[cfg(feature = "server")]
    let _thread_http = server::start();

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                let mut dirty_nodes = HashSet::new();
                for de in events {
                    for path in &de.event.paths {
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
                    println!("Change detected. Re-running tasks...");
                    let mut to_rerun = HashSet::new();
                    for start_node in &dirty_nodes {
                        let mut dfs = Dfs::new(&site.graph, *start_node);
                        while let Some(nx) = dfs.next(&site.graph) {
                            to_rerun.insert(nx);
                        }
                    }

                    run_tasks_parallel(site, &globals, &mut cache, &to_rerun)?;

                    let pages = collect_pages(&cache);
                    println!("Collected {} pages", pages.len());
                    crate::page::save_pages_to_dist(&pages).expect("Failed to save pages");
                    tx_reload.send(()).unwrap();
                    println!("Rebuild complete. Watching for changes...");
                }
            }
            Ok(Err(e)) => println!("watch error: {:?}", e),
            Err(e) => println!("watch error: {:?}", e),
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
                        eprintln!("Error: {e:?}");
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

#[cfg(feature = "server")]
mod server {
    use std::{net::SocketAddr, thread};

    use axum::Router;
    use console::style;
    use tower_http::services::ServeDir;

    pub fn start() -> thread::JoinHandle<Result<(), anyhow::Error>> {
        let port = 8080;
        let url = style(format!("http://localhost:{port}/")).yellow();
        eprintln!("Starting a HTTP server on {url}");

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
