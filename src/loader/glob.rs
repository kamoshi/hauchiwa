use camino::{Utf8Path, Utf8PathBuf};
use glob::{Pattern, glob};
use petgraph::graph::NodeIndex;
use std::{collections::HashMap, fs, sync::Arc};

use crate::{Globals, Task, loader::Registry, task::Dynamic};

pub struct GlobRegistryTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    glob_entry: &'static str,
    glob_watch: Pattern,
    callback: Box<
        dyn Fn(&Globals<G>, crate::loader::File<Vec<u8>>) -> anyhow::Result<(Utf8PathBuf, R)>
            + Send
            + Sync,
    >,
    is_dirty: bool,
}

impl<G, R> GlobRegistryTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    pub fn new<F>(glob_entry: &'static str, glob_watch: &'static str, callback: F) -> Self
    where
        F: Fn(&Globals<G>, crate::loader::File<Vec<u8>>) -> anyhow::Result<(Utf8PathBuf, R)>
            + Send
            + Sync
            + 'static,
    {
        Self {
            glob_entry,
            glob_watch: Pattern::new(glob_watch).unwrap(),
            callback: Box::new(callback),
            is_dirty: true,
        }
    }
}

impl<G, R> Task<G> for GlobRegistryTask<G, R>
where
    G: Send + Sync + 'static,
    R: Clone + Send + Sync + 'static,
{
    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn execute(&self, globals: &Globals<G>, _: &[Dynamic]) -> Dynamic {
        let mut results = Vec::new();

        for path in glob(self.glob_entry).expect("Failed to read glob pattern") {
            match path {
                Ok(path) => {
                    let path = Utf8PathBuf::try_from(path).expect("Invalid UTF-8 path");
                    let data = fs::read(&path).expect("Unable to read file");
                    let file = crate::loader::File {
                        path,
                        metadata: data,
                    };

                    let result = (self.callback)(globals, file).expect("File processing failed");
                    results.push(result);
                }
                Err(e) => eprintln!("Error processing path: {}", e),
            }
        }

        let registry = HashMap::from_iter(results.iter().cloned());
        let registry = Registry { map: registry };

        Arc::new(registry)
    }

    fn on_file_change(&mut self, path: &Utf8Path) -> bool {
        if self.glob_watch.matches_path(path.as_std_path()) {
            self.is_dirty = true;
            true
        } else {
            false
        }
    }
}
