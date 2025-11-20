use camino::{Utf8Path, Utf8PathBuf};
use glob::{Pattern, glob};
use petgraph::graph::NodeIndex;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::{collections::HashMap, fs};

use crate::{
    Globals,
    error::HauchiwaError,
    loader::Registry,
    task::{Dynamic, TypedTask},
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
    pub fn new<F>(
        glob_entry: Vec<&'static str>,
        glob_watch: Vec<&'static str>,
        callback: F,
    ) -> Result<Self, HauchiwaError>
    where
        F: Fn(&Globals<G>, crate::loader::File<Vec<u8>>) -> anyhow::Result<(Utf8PathBuf, R)>
            + Send
            + Sync
            + 'static,
    {
        Ok(Self {
            glob_entry: glob_entry.to_vec(),
            glob_watch: glob_watch
                .into_iter()
                .map(Pattern::new)
                .collect::<Result<_, _>>()?,
            callback: Box::new(callback),
        })
    }
}

impl<G, R> TypedTask<G> for GlobRegistryTask<G, R>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    type Output = Registry<R>;

    fn get_name(&self) -> String {
        self.glob_entry.join(", ")
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn execute(&self, globals: &Globals<G>, _: &[Dynamic]) -> anyhow::Result<Self::Output> {
        let mut paths = Vec::new();
        for glob_entry in &self.glob_entry {
            for path in glob(glob_entry)? {
                // Handle glob errors immediately here
                paths.push(Utf8PathBuf::try_from(path?)?);
            }
        }

        let results: anyhow::Result<Vec<_>> = paths
            .into_par_iter()
            .map(|path| {
                let data = fs::read(&path)?;

                let file = crate::loader::File {
                    path,
                    metadata: data,
                };

                (self.callback)(globals, file)
            })
            .collect();

        let registry = HashMap::from_iter(results?);
        let registry = Registry { map: registry };

        Ok(registry)
    }

    fn is_dirty(&self, path: &Utf8Path) -> bool {
        self.glob_watch.iter().any(|p| p.matches(path.as_str()))
    }
}
