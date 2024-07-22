use std::collections::HashMap;
use std::env;
use std::io::Result;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use tungstenite::WebSocket;

use crate::gen::content::build_content;
use crate::gen::styles::build_styles;
use crate::website::Source;
use crate::tree::Output;
use crate::{Artifacts, BuildContext};

pub(crate) fn watch(
	ctx: &BuildContext,
	sources: &[Source],
	mut state: Vec<Rc<Output>>,
	artifacts: &Artifacts,
) -> Result<()> {
	let root = env::current_dir().unwrap();
	let server = TcpListener::bind("127.0.0.1:1337")?;
	let client = Arc::new(Mutex::new(vec![]));

	let (tx, rx) = std::sync::mpsc::channel();
	let mut debouncer = new_debouncer(Duration::from_millis(250), tx).unwrap();

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
		let paths: Vec<Utf8PathBuf> = events
			.into_iter()
			.filter_map(|event| {
				Utf8PathBuf::from_path_buf(event.path)
					.ok()
					.and_then(|path| path.strip_prefix(&root).ok().map(ToOwned::to_owned))
			})
			.collect();

		let mut dirty = false;

		{
			let items: Vec<Rc<Output>> = paths
				.iter()
				.filter_map(|path| sources.iter().find_map(|s| s.get_maybe(path)))
				.filter_map(Option::from)
				.map(Rc::new)
				.collect();

			if !items.is_empty() {
				let state_next = update_stream(&state, &items);
				let abc: Vec<&Output> = items.iter().map(AsRef::as_ref).collect();
				let xyz: Vec<&Output> = state_next.iter().map(AsRef::as_ref).collect();
				build_content(ctx, artifacts, &abc, &xyz);
				state = state_next;
				dirty = true;
			}
		}

		if paths.iter().any(|path| path.starts_with("styles")) {
			build_styles();
			dirty = true;
		}

		if dirty {
			tx_reload.send(()).unwrap();
		}
	}

	thread_i.join().unwrap();
	thread_o.join().unwrap();

	Ok(())
}

fn update_stream(old: &[Rc<Output>], new: &[Rc<Output>]) -> Vec<Rc<Output>> {
	let mut map: HashMap<&Utf8Path, Rc<Output>> = HashMap::new();

	for output in old.iter().chain(new) {
		map.insert(&output.path, output.clone());
	}

	map.into_values().collect()
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