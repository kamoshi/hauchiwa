use camino::{Utf8Path, Utf8PathBuf};
use pagefind::api::PagefindIndex;
use pagefind::options::PagefindServiceConfig;
use tokio::runtime::Builder;

use crate::{Blueprint, Output, graph::Handle, output::OutputData};

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

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Registers a task to generate a static search index using Pagefind.
    ///
    /// This reads the provided HTML outputs and produces the necessary
    /// Pagefind Wasm and index files for client-side searching.
    ///
    /// The Wasm file is placed in the `_pagefind` directory, and the index
    /// files are placed in the `_pagefind/index` directory. Clients can use the
    /// generated `_pagefind/pagefind.js` file to perform client-side searching.
    pub fn use_pagefind(
        &mut self,
        handles: impl IntoIterator<Item = Handle<Vec<Output>>>,
    ) -> Handle<Vec<Output>> {
        let dependencies = handles.into_iter().collect::<Vec<_>>();

        self.task()
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
