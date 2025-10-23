use crate::core_structs::{Website, Sack};
use petgraph::graph::NodeIndex;
use std::sync::{Arc, Mutex};
use notify::Watcher;
use std::collections::HashSet;
use std::path::PathBuf;

impl Website {
    /// Internal helper to run a list of tasks in order.
    fn build_internal(
        &self,
        tasks_to_run: Vec<NodeIndex>,
        sack: &Arc<Mutex<Sack>>,
    ) -> Result<Vec<(PathBuf, String)>, String> {

        let mut all_files_to_write = Vec::new();

        // Iterate over the topologically-sorted build order
        for node_index in &tasks_to_run {
            let runner = self.nodes.get(node_index)
                .ok_or_else(|| "Graph is malformed".to_string())?;

            // 1. Run the task
            //    We clone the Sack's Arc, not the Sack, for the run.
            //    The run method will lock it internally.
            let output = {
                // We lock the sack *only* for fetching dependencies
                let sack_lock = sack.lock().unwrap();
                runner.run(&sack_lock)?
                // Lock is released here
            };

            // 2. Add its artifact to the Sack for downstream tasks
            let mut sack_lock = sack.lock().unwrap();
            sack_lock.add_artifact(*node_index, output.artifact, output.hash);

            // 3. Collect its files to write
            all_files_to_write.extend(output.files_to_write);
        }

        Ok(all_files_to_write)
    }

    /// Runs a full, clean build of the entire site.
    pub fn build(&self) -> Result<(), String> {
        // The shared database for this build
        let sack = Arc::new(Mutex::new(Sack::default()));

        let all_files_to_write = self.build_internal(
            self.build_order.clone(),
            &sack
        )?;

        // 4. Write all files to disk (can be parallelized)
        for (path, _content) in all_files_to_write {
            // ... fs::write(path, content) ...
            println!("[BUILD] Writing file to {:?}", path); // Placeholder
        }

        Ok(())
    }

    /// Runs the incremental watch server.
    pub fn watch(&self) -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx).unwrap();

        watcher.watch(std::path::Path::new("."), notify::RecursiveMode::Recursive).unwrap();

        // 1. Run a full clean build once to populate the Sack.
        let sack = Arc::new(Mutex::new(Sack::default()));
        self.build_internal(self.build_order.clone(), &sack)?;
        println!("[WATCH] Initial build complete. Watching for changes...");

        for res in rx {
            match res {
                Ok(event) => {
                    for path in event.paths {
                        let root_dirty_node = self.find_node_for_file(&path);
                        if let Some(root_dirty_node) = root_dirty_node {
                            let runner = self.nodes.get(&root_dirty_node).unwrap();
                            let sack_lock = sack.lock().unwrap();
                            let old_hash = sack_lock.hashes.get(&root_dirty_node).unwrap_or(&0);
                            let output = runner.run(&sack_lock).unwrap();
                            if output.hash != *old_hash {
                                let dirty_nodes = self.find_all_dependents(root_dirty_node);
                                let tasks_to_rerun: Vec<NodeIndex> = self.build_order
                                    .iter()
                                    .filter(|idx| dirty_nodes.contains(idx))
                                    .cloned()
                                    .collect();
                                println!("[WATCH] File change detected. Rerunning {} tasks.", tasks_to_rerun.len());
                                let files_to_write = self.build_internal(tasks_to_rerun, &sack)?;
                                for (path, _content) in files_to_write {
                                    println!("[WATCH] Updating file {:?}", path); // Placeholder
                                }
                            }
                        }
                    }
                }
                Err(e) => println!("watch error: {:?}", e),
            }
        }
        Ok(())
    }

    /// Finds the node corresponding to a given file path.
    fn find_node_for_file(&self, path: &std::path::Path) -> Option<NodeIndex> {
        for (glob_str, &node_index) in &self.glob_map {
            let pattern = glob::Pattern::new(glob_str).ok()?;
            if pattern.matches_path(path) {
                return Some(node_index);
            }
        }
        None
    }

    /// Internal helper to find all nodes that depend on a dirty node.
    /// This finds `start_node` and everything that depends on it,
    /// directly or indirectly.
    fn find_all_dependents(&self, start_node: NodeIndex) -> HashSet<NodeIndex> {
        let mut dirty_nodes = HashSet::new();
        let mut nodes_to_visit = vec![start_node];
        let mut visited = HashSet::new(); // To prevent re-visiting in complex graphs

        while let Some(node_to_check) = nodes_to_visit.pop() {
            if !visited.insert(node_to_check) {
                continue;
            }
            dirty_nodes.insert(node_to_check);

            // Find all nodes that depend on `node_to_check`
            // Our graph edges are `Task -> Dependency`
            // So we walk `Incoming` edges to find what depends on `node_to_check`
            for dependent in self.graph.neighbors_directed(
                node_to_check,
                petgraph::Direction::Incoming
            ) {
                nodes_to_visit.push(dependent);
            }
        }
        dirty_nodes
    }
}
