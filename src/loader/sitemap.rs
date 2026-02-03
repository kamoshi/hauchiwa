//! # Sitemap generation
//!
//! Automated generation of XML sitemaps compliant with the [Sitemap Protocol](https://www.sitemaps.org/).
//!
//! This module aggregates page outputs from your build graph to construct a
//! valid `sitemap.xml`. It handles the technical requirements of search engine
//! indexing, ensuring your content is discoverable without manual maintenance.
//!
//! ## Capabilities
//!
//! * **Auto-Scaling**: Automatically splits into a `sitemap-index.xml` and
//!   multiple sub-sitemaps if the URL count exceeds 50,000.
//! * **Dependency Integration**: Hooks directly into page generation tasks,
//!   updating the sitemap automatically when pages are added or removed.
//! * **SEO Control**: helper methods to assign `changefreq` and `priority` to
//!   different sections of your site.
//! * **Valid Output**: Uses strict XML serialization to prevent syntax errors
//!   that could confuse crawlers.
//!
//! ## Usage
//!
//! Chain the builder off the `Blueprint`, adding handles to your page collections.
//!
//! ```rust,no_run
//! use hauchiwa::{Blueprint, One, Output};
//! use hauchiwa::loader::sitemap::ChangeFrequency;
//!
//! fn configure(config: &mut Blueprint<()>) -> anyhow::Result<()> {
//!     // Assume `blog_posts` and `landing_pages` are handles from previous tasks
//!     let blog_posts: One<Vec<Output>> = todo!();
//!     let landing_pages: One<Vec<Output>>  = todo!();
//!
//!     // Generates sitemap.xml at the root of dist
//!     config.use_sitemap("https://example.org")
//!         .add(blog_posts, ChangeFrequency::Weekly, 0.8)
//!         .add(landing_pages, ChangeFrequency::Yearly, 0.5)
//!         .register();
//!
//!     Ok(())
//! }
//! ```

use camino::Utf8PathBuf;
use petgraph::graph::NodeIndex;
use sitemap_rs::sitemap::Sitemap;
use sitemap_rs::sitemap_index::SitemapIndex;
use sitemap_rs::url_set::UrlSet;

pub use sitemap_rs::url::{ChangeFrequency, Link, Url};

use std::collections::HashSet;

use crate::core::{Dynamic, Store};
use crate::engine::{TrackerPtr, TypedCoarse};
use crate::output::OutputHandle;
use crate::{Blueprint, One, Output, TaskContext, engine::Tracking};

const MAX_URLS: usize = 50_000;

// type SitemapCallback = Box<dyn Fn(&Output) -> anyhow::Result<Url> + Send + Sync>;

enum SourceStrategy {
    /// Apply a fixed frequency and priority to the default path.
    Add {
        frequency: ChangeFrequency,
        priority: f32,
    },
    // /// The user manually generates a Url.
    // Map { callback: SitemapCallback },
}

struct SitemapSource {
    index: petgraph::graph::NodeIndex,
    resolver: fn(&Dynamic) -> (Option<TrackerPtr>, Vec<&Output>),
    strategy: SourceStrategy,
}

impl SitemapSource {
    fn map_to_url(&self, base_url: &str, page: &Output) -> anyhow::Result<Url> {
        let url = match &self.strategy {
            SourceStrategy::Add {
                frequency,
                priority,
            } => Url::builder(format!("{}/{}", base_url, page.path))
                .change_frequency(*frequency)
                .priority(*priority)
                .build()?,
            // SourceStrategy::Map { callback } => callback(page)?,
        };

        Ok(url)
    }
}

/// A builder for configuring the Sitemap generation task.
pub struct SitemapBuilder<'a, G: Send + Sync> {
    blueprint: &'a mut Blueprint<G>,
    base: String,
    deps: Vec<SitemapSource>,
}

impl<'a, G: Send + Sync + 'static> SitemapBuilder<'a, G> {
    pub(crate) fn new(blueprint: &'a mut Blueprint<G>, base_url: &str) -> Self {
        Self {
            blueprint,
            base: base_url.trim_end_matches('/').to_string(),
            deps: Vec::new(),
        }
    }

    /// Registers a collection of pages for the sitemap, automatically deriving
    /// their URLs from their output file paths.
    pub fn add<H>(mut self, handle: H, frequency: ChangeFrequency, priority: f32) -> Self
    where
        H: OutputHandle,
    {
        self.deps.push(SitemapSource {
            index: handle.index(),
            resolver: H::resolve_refs,
            strategy: SourceStrategy::Add {
                frequency,
                priority,
            },
        });
        self
    }

    // /// Use this when you need custom logic (e.g., canonical URLs, specific lastmod dates).
    // pub fn map<F>(mut self, handle: Handle<Vec<Output>>, mapper: F) -> Self
    // where
    //     F: Fn(&Output) -> anyhow::Result<Url> + Send + Sync + 'static,
    // {
    //     self.deps.push(SitemapSource {
    //         index: handle.index(),
    //         strategy: SourceStrategy::Map {
    //             callback: Box::new(mapper),
    //         },
    //     });

    //     self
    // }

    pub fn register(self) -> One<Vec<Output>> {
        self.blueprint.add_task_coarse(SitemapTask {
            base_url: self.base,
            sources: self.deps,
        })
    }
}

struct SitemapTask {
    base_url: String,
    sources: Vec<SitemapSource>,
}

impl<G: Send + Sync> TypedCoarse<G> for SitemapTask {
    type Output = Vec<Output>;

    fn get_name(&self) -> String {
        "sitemap".to_string()
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
        let mut entries = Vec::new();

        for (source, input) in self.sources.iter().zip(dependencies.iter()) {
            let (tracker, items) = (source.resolver)(input);

            tracking.edges.push(tracker);

            for page in items {
                let res = source.map_to_url(&self.base_url, page)?;
                entries.push(res);
            }
        }

        entries.sort_by(|a, b| a.location.cmp(&b.location));

        // simple case when index is not needed
        if entries.len() <= MAX_URLS {
            let set = UrlSet::new(entries)?;

            let mut buffer = Vec::new();
            set.write(&mut buffer)?;

            return Ok((
                Tracking::default(),
                vec![Output::binary("sitemap.xml", buffer)],
            ));
        }

        // complex case where we need to create a sitemap index
        let base_url = self.base_url.trim_end_matches('/');
        let mut indexes = Vec::new();
        let mut outputs = Vec::new();

        for (i, chunk) in entries.chunks(MAX_URLS).enumerate() {
            let filename = format!("sitemap-{}.xml", i + 1);

            let mut buffer = Vec::new();
            UrlSet::new(chunk.to_vec())?.write(&mut buffer)?;

            outputs.push(Output::binary(&filename, buffer));

            let loc = format!("{}/{}", base_url, filename);
            let entry = Sitemap::new(loc, None);

            indexes.push(entry);
        }

        let mut buffer = Vec::new();
        SitemapIndex::new(indexes)?.write(&mut buffer)?;

        outputs.push(Output::binary("sitemap.xml", buffer));

        Ok((Tracking::default(), outputs))
    }

    fn is_valid(
        &self,
        _: &[Option<crate::engine::TrackerState>],
        _: &[Dynamic],
        updated: &HashSet<NodeIndex>,
    ) -> bool {
        !self.sources.iter().any(|s| updated.contains(&s.index))
    }
}

impl<G: Send + Sync + 'static> Blueprint<G> {
    /// Registers a task that generates a `sitemap.xml`.
    ///
    /// This method abstracts away the XML serialization. You provide the
    /// dependencies and a `mapper` function that converts those dependencies
    /// into a list of `SitemapUrl`s.
    pub fn use_sitemap(&mut self, base_url: &'static str) -> SitemapBuilder<'_, G> {
        SitemapBuilder::new(self, base_url)
    }
}
