use camino::{Utf8Path, Utf8PathBuf};
use glob::{Pattern, glob};
use petgraph::graph::NodeIndex;
use std::{collections::HashMap, fs, process::Output, sync::Arc};

use crate::{
    Globals,
    loader::Registry,
    task::{Dynamic, Task, TypedTask},
};

pub struct GlobRegistryTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    glob_entry: Vec<&'static str>,
    glob_watch: Vec<Pattern>,
    callback: Box<
        dyn Fn(&Globals<G>, crate::loader::File<Vec<u8>>) -> anyhow::Result<(Utf8PathBuf, R)>
            + Send
            + Sync,
    >,
}

impl<G, R> GlobRegistryTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    pub fn new<F>(glob_entry: Vec<&'static str>, glob_watch: Vec<&'static str>, callback: F) -> Self
    where
        F: Fn(&Globals<G>, crate::loader::File<Vec<u8>>) -> anyhow::Result<(Utf8PathBuf, R)>
            + Send
            + Sync
            + 'static,
    {
        Self {
            glob_entry: glob_entry.to_vec(),
            glob_watch: glob_watch
                .into_iter()
                .map(Pattern::new)
                .collect::<Result<_, _>>()
                .unwrap(),
            callback: Box::new(callback),
        }
    }
}

impl<G, R> TypedTask<G> for GlobRegistryTask<G, R>
where
    G: Send + Sync + 'static,
    R: Clone + Send + Sync + 'static,
{
    type Output = Registry<R>;

    fn get_name(&self) -> String {
        self.glob_entry.join(", ")
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn execute(&self, globals: &Globals<G>, _: &[Dynamic]) -> Self::Output {
        let mut results = Vec::new();

        for glob_entry in &self.glob_entry {
            for path in glob(glob_entry).expect("Failed to read glob pattern") {
                match path {
                    Ok(path) => {
                        let path = Utf8PathBuf::try_from(path).expect("Invalid UTF-8 path");
                        let data = fs::read(&path).expect("Unable to read file");
                        let file = crate::loader::File {
                            path,
                            metadata: data,
                        };

                        let result =
                            (self.callback)(globals, file).expect("File processing failed");
                        results.push(result);
                    }
                    Err(e) => eprintln!("Error processing path: {}", e),
                }
            }
        }

        let registry = HashMap::from_iter(results.iter().cloned());
        let registry = Registry { map: registry };

        registry
    }

    fn is_dirty(&self, path: &Utf8Path) -> bool {
        self.glob_watch.iter().any(|p| p.matches(path.as_str()))
    }
}
