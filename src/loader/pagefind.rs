//! # Pagefind static search indexing
//!
//! Generates a fully client-side search index using [pagefind](https://pagefind.app/).
//!
//! This module integrates Pagefind directly into the build graph, scanning your
//! generated HTML output to build a static search library (WASM/JS + Index
//! chunks). The resulting search engine runs entirely in the user's browser
//! with no backend requirement.
//!
//! ## Capabilities
//!
//! * **Automatic Ingestion**: Filters and indexes `html` outputs from upstream tasks.
//! * **Static Output**: Generates a self-contained `_pagefind` directory ready for deployment.
//! * **Zero Runtime Overhead**: Search logic is offloaded to the client's device.
//! * **Parallel Execution**: Indexing runs concurrently with other unrelated build tasks.
//!
//! ## Usage
//!
//! Feed your HTML generation handles into the builder. The task will
//! automatically collect valid HTML files and produce the search assets.
//!
//! ```rust,no_run
//! use hauchiwa::{Blueprint, One, Output};
//!
//! fn configure(config: &mut Blueprint<()>) -> anyhow::Result<()> {
//!     // 1. Define your content rendering task
//!     // let pages = ...; // One<Vec<Output>>
//!     let pages: One<Vec<Output>> = todo!();
//!
//!     // 2. Attach the search indexer
//!     // Assets will be generated at `dist/_pagefind/`
//!     config.use_pagefind()
//!         .index(pages)
//!         .register();
//!
//!     Ok(())
//! }
//! ```

use camino::{Utf8Path, Utf8PathBuf};
use pagefind::api::PagefindIndex;
use pagefind::options::PagefindServiceConfig;
use petgraph::graph::NodeIndex;
use std::collections::HashSet;
use tokio::runtime::Builder;

use crate::core::{Dynamic, Store};
use crate::engine::{TrackerPtr, Tracking, TypedCoarse};
use crate::output::{OutputData, OutputHandle};
use crate::{Blueprint, One, Output, TaskContext};

struct PagefindSource {
    index: NodeIndex,
    resolver: fn(&Dynamic) -> (Option<TrackerPtr>, Vec<&Output>),
}

/// A builder for configuring the Pagefind search index task.
pub struct PagefindBuilder<'a, G: Send + Sync> {
    blueprint: &'a mut Blueprint<G>,
    sources: Vec<PagefindSource>,
}

impl<'a, G: Send + Sync + 'static> PagefindBuilder<'a, G> {
    pub(crate) fn new(blueprint: &'a mut Blueprint<G>) -> Self {
        Self {
            blueprint,
            sources: Vec::new(),
        }
    }

    /// Adds a handle (source of HTML outputs) to the index.
    ///
    /// Accepts any handle type that resolves to a list of Outputs.
    pub fn index<H>(mut self, handle: H) -> Self
    where
        H: OutputHandle,
    {
        self.sources.push(PagefindSource {
            index: handle.index(),
            resolver: H::resolve_refs,
        });
        self
    }

    /// Registers the task with the Blueprint.
    pub fn register(self) -> One<Vec<Output>> {
        self.blueprint.add_task_coarse(PagefindTask {
            sources: self.sources,
        })
    }
}

struct PagefindTask {
    sources: Vec<PagefindSource>,
}

impl<G: Send + Sync> TypedCoarse<G> for PagefindTask {
    type Output = Vec<Output>;

    fn get_name(&self) -> String {
        "pagefind".to_string()
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        self.sources.iter().map(|s| s.index).collect()
    }

    fn get_watched(&self) -> Vec<Utf8PathBuf> {
        vec![]
    }

    fn execute(
        &self,
        _: &TaskContext<G>,
        _: &mut Store,
        dependencies: &[Dynamic],
    ) -> anyhow::Result<(Tracking, Self::Output)> {
        let mut tracking = Tracking::default();
        let mut pages_to_index = Vec::new();

        // 1. Resolve all dependencies and collect valid HTML files
        for (source, input) in self.sources.iter().zip(dependencies.iter()) {
            let (tracker, items) = (source.resolver)(input);

            tracking.edges.push(tracker);

            for page in items {
                // Filter specifically for HTML files containing Utf8 data
                if let OutputData::Utf8(data) = &page.data
                    && matches!(page.path.extension(), Some("htm") | Some("html"))
                {
                    pages_to_index.push((page.path.to_string(), data.clone()));
                }
            }
        }

        // 2. Run Pagefind (Requires an async runtime)
        // Since execute is synchronous, we spin up a lightweight local runtime.
        let output = Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async move {
                let config = PagefindServiceConfig::builder().build();
                let mut index = PagefindIndex::new(Some(config))?;

                for (path, content) in pages_to_index {
                    index.add_html_file(Some(path), None, content).await?;
                }

                // Generate the index chunks and WASM bindings
                index.build_indexes().await?;
                let files = index.get_files().await?;

                let mut artifacts = Vec::new();
                for file in files {
                    // Pagefind returns relative paths like "pagefind.js"
                    // We prefix them to place them in the correct output dir.
                    let path = Utf8PathBuf::try_from(file.filename)?;
                    let path = Utf8Path::new("_pagefind").join(path);
                    artifacts.push(Output::binary(path, file.contents));
                }

                Ok::<_, anyhow::Error>(artifacts)
            })?;

        Ok((tracking, output))
    }

    fn is_valid(
        &self,
        _: &[Option<crate::engine::TrackerState>],
        _: &[Dynamic],
        updated: &HashSet<NodeIndex>,
    ) -> bool {
        // If any source dependency has changed, we must rebuild the index.
        !self.sources.iter().any(|s| updated.contains(&s.index))
    }
}

impl<G: Send + Sync + 'static> Blueprint<G> {
    /// Initiates the configuration for a static search index using Pagefind.
    pub fn use_pagefind(&mut self) -> PagefindBuilder<'_, G> {
        PagefindBuilder::new(self)
    }
}
