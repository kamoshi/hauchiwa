use std::{net::SocketAddr, thread};

use axum::Router;
use console::style;
use tower_http::services::ServeDir;
use tracing::info;

pub fn start(dist: String) -> thread::JoinHandle<Result<(), anyhow::Error>> {
    let port = 8080;

    info!(url = %style(format!("http://localhost:{port}/")).yellow(), "starting a HTTP server");

    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(serve(port, dist))
    })
}

async fn serve(port: u16, dist: String) -> Result<(), anyhow::Error> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let address = tokio::net::TcpListener::bind(address).await?;

    let router = Router::new()
        .fallback_service(ServeDir::new(dist));

    axum::serve(address, router).await?;

    Ok(())
}
