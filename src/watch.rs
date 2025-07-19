use std::collections::HashSet;
use std::env;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::new_debouncer;
use tungstenite::WebSocket;

use crate::build;
use crate::error::WatchError;
use crate::loader::Loadable;
use crate::{Globals, Mode, Website, init};

fn reserve_port() -> Result<(TcpListener, u16), WatchError> {
    let listener = match TcpListener::bind("127.0.0.1:1337") {
        Ok(sock) => sock,
        Err(_) => TcpListener::bind("127.0.0.1:0").map_err(WatchError::Bind)?,
    };

    let addr = listener.local_addr().map_err(WatchError::Bind)?;
    let port = addr.port();
    Ok((listener, port))
}

pub fn watch<G>(website: &mut Website<G>, data: G) -> anyhow::Result<()>
where
    G: Send + Sync + 'static,
{
    let root = env::current_dir()?;
    let (tcp, port) = reserve_port()?;
    let client = Arc::new(Mutex::new(vec![]));

    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = new_debouncer(Duration::from_millis(250), None, tx)?;

    for base in website
        .loaders
        .iter()
        .map(Loadable::path_base)
        .collect::<HashSet<_>>()
    {
        debouncer.watch(Path::new(base), RecursiveMode::Recursive)?;
    }

    let thread_i = new_thread_ws_incoming(tcp, client.clone());
    let (tx_reload, thread_o) = new_thread_ws_reload(client.clone());

    let globals = Globals {
        mode: Mode::Watch,
        port: Some(port),
        data,
    };

    init(website)?;
    build(website, &globals)?;

    #[cfg(feature = "server")]
    let thread_http = server::start();

    while let Ok(events) = rx.recv()? {
        let mut dirty = false;

        let obsolete = match events
            .iter()
            .filter(|de| {
                matches!(
                    de.event.kind,
                    EventKind::Create(..) | EventKind::Modify(..) | EventKind::Remove(..)
                )
            })
            .flat_map(|de| &de.event.paths)
            .try_fold(
                HashSet::new(),
                |mut acc, path| -> Result<_, anyhow::Error> {
                    let path = path.strip_prefix(&root)?;
                    let path = Utf8PathBuf::try_from(path.to_path_buf())?;
                    acc.insert(path);
                    Ok(acc)
                },
            ) {
            Ok(ok) => ok,
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        };

        let modified = match events
            .iter()
            .filter(|de| {
                matches!(
                    de.event.kind,
                    EventKind::Create(..) | EventKind::Modify(..) | EventKind::Remove(..)
                )
            })
            .flat_map(|de| &de.event.paths)
            .filter(|path| path.exists())
            .try_fold(
                HashSet::new(),
                |mut acc, path| -> Result<_, anyhow::Error> {
                    let path = path.strip_prefix(&root)?;
                    let path = Utf8PathBuf::try_from(path.to_path_buf())?;
                    acc.insert(path);
                    Ok(acc)
                },
            ) {
            Ok(ok) => ok,
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        };

        if obsolete.is_empty() && modified.is_empty() {
            continue;
        }

        if !obsolete.is_empty() {
            dirty |= website.loaders_remove(&obsolete);
        }

        if !modified.is_empty() {
            dirty |= match website.loaders_reload(&modified) {
                Ok(ok) => ok,
                Err(e) => {
                    eprintln!("Error while reloading:\n{e}");
                    continue;
                }
            };
        }

        if dirty {
            let start = Instant::now();

            match build(website, &globals) {
                Ok(()) => tx_reload.send(())?,
                Err(e) => {
                    eprintln!("Encountered an error while rebuilding: {e}")
                }
            };

            let duration = start.elapsed();
            println!("Refreshed in {duration:?}");
        }
    }

    thread_i.join().unwrap();
    thread_o.join().unwrap();

    #[cfg(feature = "server")]
    thread_http.join().unwrap().unwrap();

    Ok(())
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
