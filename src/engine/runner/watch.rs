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

use crate::engine::{run_once_parallel, run_tasks_parallel};
use crate::{Environment, Mode, Website};

use std::collections::HashSet;
use std::env;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
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

    tracing::info!("running initial build...");
    let (mut cache, pages, _diagnostics) = run_once_parallel(site, &globals)?;
    tracing::info!("collected {} pages", pages.len());
    crate::output::save_pages_to_dist(&pages).expect("Failed to save pages");

    tracing::info!("initial build completed, now watching for changes...");
    let clients = Arc::new(Mutex::new(vec![]));

    let _thread_i = new_thread_ws_incoming(tcp, clients.clone());
    let (tx_reload, _thread_o) = new_thread_ws_reload(clients.clone());

    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = new_debouncer(Duration::from_millis(250), None, tx).unwrap();

    let mut watched = HashSet::new();
    let mut filters = HashSet::new();
    for (_, task) in site.graph.node_references() {
        for path in &task.watched() {
            if let Ok((path, pattern)) = resolve_watch_path(path) {
                watched.insert(path);
                filters.insert(pattern);
            } else {
                tracing::error!("failed to resolve path: {}", &path);
            };
        }
    }

    // Collapse watched paths to reduce the number of watches
    let watched = collapse_watch_paths(watched);

    for path in watched {
        tracing::info!("watching {}", path);
        debouncer.watch(path, RecursiveMode::Recursive)?;
    }

    #[cfg(feature = "server")]
    let _thread_http = super::http::start();

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                tracing::info!("{:?} events received", events);

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
                    tracing::info!("change detected, re-running tasks...");
                    let mut to_rerun = HashSet::new();
                    for start_node in &dirty_nodes {
                        let mut dfs = petgraph::visit::Dfs::new(&site.graph, *start_node);
                        while let Some(nx) = dfs.next(&site.graph) {
                            to_rerun.insert(nx);
                        }
                    }

                    let _diagnostics = match run_tasks_parallel(
                        site,
                        &globals,
                        &mut cache,
                        &to_rerun,
                        &dirty_nodes,
                    ) {
                        Ok(res) => res,
                        Err(e) => {
                            tracing::error!("Error running tasks: {}", e);
                            continue;
                        }
                    };

                    let pages = super::collect_pages(&cache);
                    tracing::info!("collected {} pages", pages.len());
                    crate::output::save_pages_to_dist(&pages).expect("Failed to save pages");
                    tx_reload.send(()).unwrap();
                    tracing::info!("rebuild complete, watching for changes...");
                }
            }
            Ok(Err(e)) => tracing::error!("watch error: {:?}", e),
            Err(e) => tracing::error!("watch error: {:?}", e),
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
                        tracing::error!("Error: {e:?}");
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
