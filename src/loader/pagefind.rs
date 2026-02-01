use camino::{Utf8Path, Utf8PathBuf};
use pagefind::api::PagefindIndex;
use pagefind::options::PagefindServiceConfig;
use tokio::runtime::Builder;

use crate::{Blueprint, One, Output, output::OutputData};

async fn build_closure(pages: &[&Output]) -> Result<Vec<Output>, anyhow::Error> {
    let config = PagefindServiceConfig::builder().build();
    let mut index = PagefindIndex::new(Some(config))?;

    for page in pages {
        if let OutputData::Utf8(data) = &page.data
            && matches!(page.path.extension(), Some("htm") | Some("html"))
        {
            index
                .add_html_file(Some(page.path.to_string()), None, data.clone())
                .await?;
        }
    }

    index.build_indexes().await?;

    let mut output = Vec::new();
    for file in index.get_files().await? {
        let path = Utf8PathBuf::try_from(file.filename)?;
        let path = Utf8Path::new("_pagefind").join(path);

        output.push(Output::binary(path, file.contents));
    }

    Ok(output)
}

pub struct PagefindBuilder<'a, G>
where
    G: Send + Sync,
{
    blueprint: &'a mut Blueprint<G>,
    handles: Vec<One<Vec<Output>>>,
}

impl<'a, G> PagefindBuilder<'a, G>
where
    G: Send + Sync + 'static,
{
    /// Adds a handle (source of HTML outputs) to the index.
    pub fn index(mut self, handle: One<Vec<Output>>) -> Self {
        self.handles.push(handle);
        self
    }

    /// Consumes the builder and registers the task with the Blueprint.
    pub fn register(self) -> One<Vec<Output>> {
        let dependencies = self.handles.clone();

        self.blueprint
            .task()
            .name("pagefind")
            .depends_on(dependencies)
            .run(|_, handles| {
                let pages = handles
                    .into_iter()
                    .flat_map(|source| source.iter())
                    .collect::<Vec<_>>();

                let output = Builder::new_multi_thread()
                    .enable_all()
                    .build()?
                    .block_on(build_closure(&pages))?;

                Ok(output)
            })
    }
}

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Initiates the configuration for a static search index using Pagefind.
    ///
    /// Returns a builder that allows adding multiple sources via `.index()`.
    /// Call `.register()` to finalize the task.
    pub fn use_pagefind(&mut self) -> PagefindBuilder<'_, G> {
        PagefindBuilder {
            blueprint: self,
            handles: Vec::new(),
        }
    }
}
