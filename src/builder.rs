
use std::collections::HashMap;
use std::sync::Arc;
use std::marker::PhantomData;
use std::path::PathBuf;
use crate::core_structs::*;
use crate::task_deps::TaskDependencies;
use petgraph::graph::NodeIndex;

// Placeholder for the actual CollectionRunner struct
struct CollectionRunner<T, F> {
    glob: &'static str,
    processor: Arc<F>,
    _marker: PhantomData<T>,
}

use glob::glob;
use std::fs;
// Placeholder impl for TaskRunner
impl<T, F> TaskRunner for CollectionRunner<T, F>
where
    T: 'static + Send + Sync,
    F: Fn(PathBuf, Vec<u8>) -> T + 'static + Send + Sync,
{
    fn run(&self, _sack: &Sack) -> Result<TaskOutput, String> {
        let mut collection_data: Vec<T> = Vec::new();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for entry in glob(self.glob).map_err(|e| format!("Invalid glob pattern: {}", e))?.filter_map(Result::ok) {
            let bytes = fs::read(&entry).map_err(|e| format!("Failed to read file {:?}: {}", entry, e))?;
            std::hash::Hash::hash_slice(&bytes, &mut hasher);
            let path_buf = PathBuf::from(entry);
            collection_data.push((self.processor)(path_buf, bytes));
        }

        let hash = std::hash::Hasher::finish(&hasher);
        Ok(TaskOutput {
            files_to_write: vec![], // Collections don't write files
            artifact: Box::new(collection_data),
            hash,
        })
    }

    fn get_dependencies(&self) -> Vec<NodeIndex> {
        vec![] // Collections are source nodes, they have no dependencies
    }
}

// Placeholder for the actual GenericTaskRunner struct
struct GenericTaskRunner<Deps, F, In, Out> {
    dependencies: Deps,
    task_fn: Arc<F>,
    _marker: PhantomData<(In, Out)>,
}

// Placeholder impl for TaskRunner
impl<Deps, F, In, Out> TaskRunner for GenericTaskRunner<Deps, F, In, Out>
where
    Deps: TaskDependencies<ResolvedData = In>,
    F: Fn(In) -> (Vec<(PathBuf, String)>, Out) + 'static + Send + Sync,
    In: 'static + Send + Sync,
    Out: 'static + Send + Sync,
{
    fn run(&self, sack: &Sack) -> Result<TaskOutput, String> {
        // 1. Resolve dependencies from the Sack
        let resolved_data: In = self.dependencies.resolve(sack)?;

        // 2. Run the user's closure
        let (files_to_write, artifact_data) = (self.task_fn)(resolved_data);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for dep in self.dependencies.get_indices() {
            let hash = sack.hashes.get(&dep).unwrap_or(&0);
            std::hash::Hash::hash(&hash, &mut hasher);
        }
        let hash = std::hash::Hasher::finish(&hasher);

        Ok(TaskOutput {
            files_to_write,
            artifact: Box::new(artifact_data),
            hash,
        })
    }

    fn get_dependencies(&self) -> Vec<NodeIndex> {
        self.dependencies.get_indices()
    }
}


impl WebsiteBuilder {
    pub fn new() -> Self {
        WebsiteBuilder {
            graph: DependencyGraph::default(),
            nodes: HashMap::new(),
            glob_map: HashMap::new(),
        }
    }
    /// Adds a collection (a "source" node) to the graph.
    pub fn add_collection<T, F>(
        &mut self,
        glob: &'static str, // The glob pattern to watch
        processor: F,       // The user's function: `(path, bytes) -> T`
    ) -> CollectionHandle<T>
    where
        T: 'static + Send + Sync,
        F: Fn(PathBuf, Vec<u8>) -> T + 'static + Send + Sync,
    {
        // 1. Create the runner logic for this collection
        let runner = CollectionRunner {
            glob,
            processor: Arc::new(processor), // Use Arc for sharing
            _marker: PhantomData,
        };

        // 2. Add a new node to the graph for this collection
        let node_index = self.graph.add_node(());
        self.glob_map.insert(glob, node_index);

        // 3. Store the type-erased runner
        self.nodes.insert(node_index, Box::new(runner));

        // 4. Return the type-safe handle
        Handle { index: node_index, _marker: PhantomData }
    }

    /// Adds a task (a "computation" node) to the graph.
    pub fn add_task<Deps, F, In, Out>(
        &mut self,
        dependencies: Deps, // The tuple of handles
        task_fn: F,         // The user's closure
    ) -> ArtifactHandle<Out>
    where
        Deps: TaskDependencies<ResolvedData = In>,
        F: Fn(In) -> (Vec<(PathBuf, String)>, Out) + 'static + Send + Sync,
        In: 'static + Send + Sync,
        Out: 'static + Send + Sync,
    {
        // 1. Create the runner logic for this task
        let runner = GenericTaskRunner {
            dependencies,
            task_fn: Arc::new(task_fn), // Use Arc for sharing
            _marker: PhantomData,
        };

        // 2. Add a new node to the graph for this task
        let node_index = self.graph.add_node(());

        // 3. Add EDGES to the graph for each dependency
        for dep_index in runner.get_dependencies() {
            // Edge: This Task (node_index) -> Depends On (dep_index)
            self.graph.add_edge(node_index, dep_index, ());
        }

        // 5. Store the type-erased runner
        self.nodes.insert(node_index, Box::new(runner));

        // 6. Return a handle to this task's *artifact*
        Handle { index: node_index, _marker: PhantomData }
    }

    /// Finishes the build setup, validates the graph, and returns an
    /// executable Website.
    pub fn finish(self) -> Result<Website, String> {
        // 1. Check for circular dependencies
        if petgraph::algo::is_cyclic_directed(&self.graph) {
            return Err("Circular dependency detected in build graph".to_string());
        }

        // 2. Get the topological build order
        let build_order = petgraph::algo::toposort(&self.graph, None)
            .map_err(|e| format!("Failed to get build order: {:?}", e))?;

        // Note: toposort gives an order from dependencies -> dependents.
        // For our build, we need to execute dependencies *first*, so we
        // must reverse this list.
        let build_order_reversed = build_order.into_iter().rev().collect();

        Ok(Website {
            graph: self.graph,
            nodes: self.nodes,
            build_order: build_order_reversed,
            glob_map: self.glob_map,
        })
    }
}
