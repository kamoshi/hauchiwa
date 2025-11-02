use std::{
    collections::{HashMap, HashSet},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, mpsc::Sender},
    thread::JoinHandle,
    time::Duration,
};

use camino::Utf8Path;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use notify::RecursiveMode;
use notify_debouncer_full::new_debouncer;
use petgraph::graph::NodeIndex;
use petgraph::{algo::toposort, visit::Dfs};
use crossbeam_channel::unbounded;
use tungstenite::WebSocket;

use crate::{page::Page, task::Dynamic, Globals, Mode, Site, Task};

pub fn run_once_parallel<G: Send + Sync>(
    site: &Site<G>,
    globals: &Globals<G>,
) -> (HashMap<NodeIndex, Dynamic>, Vec<Page>) {
    let mut cache: HashMap<NodeIndex, Dynamic> = HashMap::new();
    toposort(&site.graph, None).expect("Cycle detected in task graph");

    // Build a map from a dependency to the nodes that depend on it.
    let mut dependents: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
    for edge in site.graph.raw_edges() {
        dependents
            .entry(edge.source())
            .or_default()
            .push(edge.target());
    }

    // Count dependencies for each node
    let mut dependency_counts: HashMap<NodeIndex, usize> = site
        .graph
        .node_indices()
        .map(|i| {
            (
                i,
                site.graph
                    .neighbors_directed(i, petgraph::Direction::Incoming)
                    .count(),
            )
        })
        .collect();

    let total_tasks = site.graph.node_count();
    let mut completed_tasks = 0;

    // Setup MultiProgress and the main overall progress bar
    let mp = MultiProgress::new();
    let main_pb = mp.add(ProgressBar::new(total_tasks as u64));
    main_pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}",
            )
            .unwrap()
            .progress_chars("=>-"),
    );
    main_pb.set_message("Building tasks...");

    let (task_sender, task_receiver) =
        unbounded::<(NodeIndex, Box<dyn Task<G>>, Vec<Dynamic>)>();
    let (result_sender, result_receiver) = unbounded::<(NodeIndex, Dynamic)>();

    std::thread::scope(|s| {
        let num_workers = std::thread::available_parallelism().unwrap().get();

        // Spawn workers
        for _ in 0..num_workers {
            let task_receiver = task_receiver.clone();
            let result_sender = result_sender.clone();
            s.spawn(move || {
                while let Ok((node_index, task, inputs)) = task_receiver.recv() {
                    let output = task.execute(globals, &inputs);
                    result_sender.send((node_index, output)).unwrap();
                }
            });
        }
        drop(task_receiver); // Drop original receiver, workers have clones
        drop(result_sender);

        // Find initial tasks (those with no dependencies) and send them to the workers
        for (&node_index, &count) in &dependency_counts {
            if count == 0 {
                let task = site.graph[node_index].clone_box();
                task_sender.send((node_index, task, vec![])).unwrap();
            }
        }

        // Main scheduler loop
        while completed_tasks < total_tasks {
            let (completed_index, output) = result_receiver.recv().unwrap();

            cache.insert(completed_index, output);
            completed_tasks += 1;
            main_pb.inc(1);

            // Check if any other tasks are now ready to run
            if let Some(dependents_of_completed) = dependents.get(&completed_index) {
                for &dependent_index in dependents_of_completed {
                    let count = dependency_counts.get_mut(&dependent_index).unwrap();
                    *count -= 1;
                    if *count == 0 {
                        // All dependencies met, this task is ready
                        let dependencies = site.graph[dependent_index]
                            .dependencies()
                            .iter()
                            .map(|dep_index| cache.get(dep_index).unwrap().clone())
                            .collect::<Vec<_>>();
                        let task = site.graph[dependent_index].clone_box();
                        task_sender
                            .send((dependent_index, task, dependencies))
                            .unwrap();
                    }
                }
            }
        }
    });

    main_pb.finish_with_message("Build complete!");
    let pages = collect_pages(&cache);
    (cache, pages)
}

pub fn run_once<G: Send + Sync>(
    site: &mut Site<G>,
    globals: &Globals<G>,
) -> (HashMap<NodeIndex, Dynamic>, Vec<Page>) {
    let mut cache: HashMap<NodeIndex, Dynamic> = HashMap::new();

    let sorted_nodes = toposort(&site.graph, None).unwrap();

    for node_index in sorted_nodes {
        let task = site.graph.node_weight(node_index).unwrap();
        let dependencies = task.dependencies();
        let dependency_outputs: Vec<Dynamic> = dependencies
            .iter()
            .map(|dep_index| cache.get(dep_index).unwrap().clone())
            .collect();

        let output = task.execute(globals, &dependency_outputs);
        cache.insert(node_index, output);
    }

    let pages = collect_pages(&cache);
    (cache, pages)
}

fn collect_pages(cache: &HashMap<NodeIndex, Dynamic>) -> Vec<Page> {
    let mut pages: Vec<Page> = Vec::new();
    for value in cache.values() {
        if let Some(page) = value.downcast_ref::<Page>() {
            pages.push(page.clone());
        } else if let Some(page_vec) = value.downcast_ref::<Vec<Page>>() {
            pages.extend(page_vec.clone());
        }
    }
    pages
}

pub fn watch<G: Send + Sync + Clone + 'static>(site: &mut Site<G>, data: G) {
    let (tcp, port) = reserve_port().unwrap();
    let globals = Globals {
        generator: "hauchiwa",
        mode: Mode::Watch,
        port: Some(port),
        data,
    };

    println!("Performing initial build...");
    let (mut cache, pages) = run_once(site, &globals);
    println!("Collected {} pages", pages.len());
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
                            for index in site.graph.node_indices() {
                                let task = site.graph.node_weight_mut(index).unwrap();
                                if task.on_file_change(path) {
                                    dirty_nodes.insert(index);
                                }
                            }
                        }
                    }
                }

                if !dirty_nodes.is_empty() {
                    println!("Change detected. Re-running tasks...");

                    // Find all dependents of the dirty nodes
                    let mut to_rerun = HashSet::new();
                    for start_node in &dirty_nodes {
                        let mut dfs = Dfs::new(&site.graph, *start_node);
                        while let Some(nx) = dfs.next(&site.graph) {
                            to_rerun.insert(nx);
                        }
                    }

                    let sorted_nodes = toposort(&site.graph, None).unwrap();
                    for node_index in sorted_nodes {
                        if to_rerun.contains(&node_index) {
                            let task = site.graph.node_weight(node_index).unwrap();
                            let dependencies = task.dependencies();
                            let dependency_outputs: Vec<Dynamic> = dependencies
                                .iter()
                                .map(|dep_index| cache.get(dep_index).unwrap().clone())
                                .collect();

                            let output = task.execute(&globals, &dependency_outputs);
                            cache.insert(node_index, output);
                        }
                    }

                    let pages = collect_pages(&cache);
                    println!("Collected {} pages", pages.len());
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
    use petgraph::graph::NodeIndex;
    use std::sync::Arc;

    #[test]
    fn test_collect_pages() {
        let mut cache: HashMap<NodeIndex, Dynamic> = HashMap::new();
        let page1 = Page {
            url: "/".into(),
            content: "Home".to_string(),
        };
        let page2 = Page {
            url: "/about".into(),
            content: "About".to_string(),
        };
        let page3 = Page {
            url: "/contact".into(),
            content: "Contact".to_string(),
        };

        cache.insert(NodeIndex::new(0), Arc::new(page1.clone()) as Dynamic);
        cache.insert(
            NodeIndex::new(1),
            Arc::new(vec![page2.clone(), page3.clone()]) as Dynamic,
        );
        cache.insert(
            NodeIndex::new(2),
            Arc::new("not a page".to_string()) as Dynamic,
        );

        let pages = collect_pages(&cache);

        assert_eq!(pages.len(), 3);
        assert!(pages.iter().any(|p| p.url == "/"));
        assert!(pages.iter().any(|p| p.url == "/about"));
        assert!(pages.iter().any(|p| p.url == "/contact"));
    }
}
