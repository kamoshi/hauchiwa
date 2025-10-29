use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use camino::Utf8Path;
use notify::{RecursiveMode, Watcher};
use petgraph::{algo::toposort, visit::Dfs};
use petgraph::{graph::NodeIndex, Direction};

use crate::{task::Dynamic, Site};

pub fn run_once(site: &mut Site) -> HashMap<NodeIndex, Dynamic> {
    let mut cache: HashMap<NodeIndex, Dynamic> = HashMap::new();

    let sorted_nodes = toposort(&site.graph, None).unwrap();

    for node_index in sorted_nodes {
        let task = site.graph.node_weight(node_index).unwrap();
        let dependencies = task.dependencies();
        let dependency_outputs: Vec<Dynamic> = dependencies
            .iter()
            .map(|dep_index| cache.get(dep_index).unwrap().clone())
            .collect();

        let output = task.execute(&dependency_outputs);
        cache.insert(node_index, output);
    }

    cache
}

pub fn watch(site: &mut Site) {
    println!("Performing initial build...");
    let mut cache = run_once(site);
    println!("Initial build complete. Watching for changes...");

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx).unwrap();
    watcher
        .watch(Utf8Path::new(".").as_std_path(), RecursiveMode::Recursive)
        .unwrap();

    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(event)) => {
                let mut dirty_nodes = HashSet::new();

                for path in &event.paths {
                    if let Some(path) = Utf8Path::from_path(path) {
                        for index in site.graph.node_indices() {
                            let task = site.graph.node_weight_mut(index).unwrap();
                            if task.on_file_change(path) {
                                dirty_nodes.insert(index);
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

                            let output = task.execute(&dependency_outputs);
                            cache.insert(node_index, output);
                        }
                    }
                    println!("Rebuild complete. Watching for changes...");
                }
            }
            Ok(Err(e)) => println!("watch error: {:?}", e),
            Err(_) => {
                // Timeout, nothing to do
            }
        }
    }
}
