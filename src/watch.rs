use std::collections::HashSet;
use std::env;
use std::io::Result;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::new_debouncer;
use tungstenite::WebSocket;

use crate::builder::Scheduler;
use crate::Website;

pub(crate) fn watch<G: Send + Sync + 'static>(
	website: &Website<G>,
	mut scheduler: Scheduler<G>,
) -> Result<()> {
	let root = env::current_dir().unwrap();
	let server = TcpListener::bind("127.0.0.1:1337")?;
	let client = Arc::new(Mutex::new(vec![]));

	let (tx, rx) = std::sync::mpsc::channel();
	let mut debouncer = new_debouncer(Duration::from_millis(250), None, tx).unwrap();

	debouncer
		.watcher()
		.watch(Path::new("styles"), RecursiveMode::Recursive)
		.unwrap();

	debouncer
		.watcher()
		.watch(Path::new("content"), RecursiveMode::Recursive)
		.unwrap();

	let thread_i = new_thread_ws_incoming(server, client.clone());
	let (tx_reload, thread_o) = new_thread_ws_reload(client.clone());

	while let Ok(events) = rx.recv().unwrap() {
		let paths: HashSet<Utf8PathBuf> = events
			.into_iter()
			.map(|debounced| debounced.event.paths)
			.flat_map(HashSet::<PathBuf>::from_iter)
			.filter_map(|event| {
				Utf8PathBuf::from_path_buf(event)
					.ok()
					.and_then(|path| path.strip_prefix(&root).ok().map(ToOwned::to_owned))
			})
			.collect();

		let mut dirty = false;
		if paths.iter().any(|path| path.starts_with("styles")) {
			let new_items = crate::generator::load_styles(&website.global_styles);

			scheduler.update(new_items);
			dirty = true;
		}

		if paths.iter().any(|path| path.starts_with("content")) {
			let new_items = paths
				.iter()
				.filter_map(|path| {
					website
						.collections
						.iter()
						.find_map(|collection| collection.load_single(path))
				})
				.collect();

			scheduler.update(new_items);
			dirty = true;
		}

		if dirty {
			println!("\nStarting rebuild...");
			let start = Instant::now();

			scheduler.build();
			tx_reload.send(()).unwrap();

			let duration = start.elapsed();
			println!("Finished rebuild in {duration:?}");
		}
	}

	thread_i.join().unwrap();
	thread_o.join().unwrap();

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
						eprintln!("Error: {:?}", e);
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
