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
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tungstenite::WebSocket;

use crate::{Globals, Mode, Site, page::Page, task::Dynamic};

pub fn run_once_parallel<G: Send + Sync>(
    site: &mut Site<G>,
    globals: &Globals<G>,
) -> (HashMap<NodeIndex, Dynamic>, Vec<Page>) {
    let mut cache: HashMap<NodeIndex, Dynamic> = HashMap::new();
    let mut pending_nodes: HashSet<NodeIndex> = site.graph.node_indices().collect();

    // We run toposort primarily to detect any cycles in the graph.
    toposort(&site.graph, None).expect("Cycle detected in task graph");

    // Setup MultiProgress and the main overall progress bar
    let mp = MultiProgress::new();
    let total_tasks = site.graph.node_count() as u64;
    let main_pb = mp.add(ProgressBar::new(total_tasks));
    main_pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    main_pb.set_message("Building tasks...");

    // We clone this style for each new spinner inside the parallel map.
    let spinner_style = ProgressStyle::default_spinner()
        .template("{spinner:.blue} {msg}")
        .unwrap();

    while !pending_nodes.is_empty() {
        let ready_nodes: Vec<NodeIndex> = pending_nodes
            .iter()
            .filter(|&&node_index| {
                site.graph[node_index]
                    .dependencies()
                    .iter()
                    .all(|dep| cache.contains_key(dep))
            })
            .cloned()
            .collect();

        if ready_nodes.is_empty() && !pending_nodes.is_empty() {
            // This should not happen in a valid DAG.
            main_pb.abandon_with_message("Execution deadlock!");
            panic!("Execution deadlock: no tasks are ready to run, but tasks are still pending.");
        }

        let new_results: Vec<(NodeIndex, Dynamic)> = ready_nodes
            .par_iter()
            .map(|&node_index| {
                // Create a new spinner for this specific task
                let task_pb = mp.add(ProgressBar::new_spinner());
                task_pb.set_style(spinner_style.clone());
                task_pb.enable_steady_tick(Duration::from_millis(100));

                let task = &site.graph[node_index];

                // Set the spinner's message to the task name
                task_pb.set_message(format!("Running: {}", task.get_name()));

                // Grab the inputs for this task
                let inputs = task
                    .dependencies()
                    .iter()
                    .map(|dep_index| cache.get(dep_index).unwrap().clone())
                    .collect::<Vec<_>>();

                // This is where the actual work happens
                let output = task.execute(globals, &inputs);

                // Task is finished, remove its spinner
                task_pb.finish_and_clear();

                (node_index, output)
            })
            .collect();

        for (node_index, output) in new_results {
            cache.insert(node_index, output);
            pending_nodes.remove(&node_index);

            // Increment the main overall progress bar
            main_pb.inc(1);
        }
    }

    // Mark the main progress bar as complete
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
    let (tx_reload, thread_o) = new_thread_ws_reload(clients.clone());

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
