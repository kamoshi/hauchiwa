use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tungstenite::WebSocket;

use crate::build;
use crate::error::WatchError;
use crate::{Globals, Mode, Website};

fn reserve_port() -> Result<(TcpListener, u16), WatchError> {
    let listener = match TcpListener::bind("127.0.0.1:1337") {
        Ok(sock) => sock,
        Err(_) => TcpListener::bind("127.0.0.1:0")?,
    };

    let addr = listener.local_addr()?;
    let port = addr.port();
    Ok((listener, port))
}

pub fn watch<G>(website: &mut Website<G>, data: G) -> Result<(), WatchError>
where
    G: Send + Sync + 'static,
{
    let (_tcp, port) = reserve_port()?;
    let client = Arc::new(Mutex::new(vec![]));

    let (_tx, rx) = std::sync::mpsc::channel::<Result<notify::Event, notify::Error>>();

    let (_tx_reload, thread_o) = new_thread_ws_reload(client.clone());

    let globals = Globals {
        mode: Mode::Watch,
        port: Some(port),
        data,
    };

    build(website, &globals)?;

    #[cfg(feature = "server")]
    let thread_http = server::start();

    while let Ok(_events) = rx.recv()? {

    }

    thread_o.join().unwrap();

    #[cfg(feature = "server")]
    thread_http.join().unwrap().unwrap();

    Ok(())
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
