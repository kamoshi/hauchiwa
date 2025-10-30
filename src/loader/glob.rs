use crate::{
    task::{Dynamic},
    Globals, Task,
};
use camino::{Utf8Path, Utf8PathBuf};
use ::glob::{glob, Pattern};
use petgraph::graph::NodeIndex;
use std::{fs, sync::Arc};

pub struct GlobLoaderTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    entry_glob: &'static str,
    watch_pattern: Pattern,
    callback: Box<dyn Fn(&Globals<G>, crate::loader::File<Vec<u8>>) -> anyhow::Result<R> + Send + Sync>,
    is_dirty: bool,
    _phantom: std::marker::PhantomData<G>,
}

impl<G, R> GlobLoaderTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    pub fn new<F>(entry_glob: &'static str, watch_glob: &'static str, callback: F) -> Self
    where
        F: Fn(&Globals<G>, crate::loader::File<Vec<u8>>) -> anyhow::Result<R> + Send + Sync + 'static,
    {
        let watch_pattern = Pattern::new(watch_glob).unwrap();
        Self {
            entry_glob,
            watch_pattern,
            callback: Box::new(callback),
            is_dirty: true,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<G, R> Task<G> for GlobLoaderTask<G, R>
where
    G: Send + Sync + 'static,
    R: Clone + Send + Sync + 'static,
{
    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn execute(&self, globals: &Globals<G>, _dependencies: &[Dynamic]) -> Dynamic {
        let mut results = Vec::new();
        for path in glob(self.entry_glob).expect("Failed to read glob pattern") {
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
        Arc::new(results)
    }

    fn on_file_change(&mut self, path: &Utf8Path) -> bool {
        if self.watch_pattern.matches_path(path.as_std_path()) {
            self.is_dirty = true;
            true
        } else {
            false
        }
    }
}
