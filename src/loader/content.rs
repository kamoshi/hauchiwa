use std::sync::LazyLock;

use gray_matter::engine::{JSON, YAML};

use crate::{
    loader::{File, FileLoaderTask},
    task::Handle,
    SiteConfig,
};

/// This is the canonical in-memory representation for markdown, or any textual
/// content files parsed via front matter. Used as the payload type for
/// [`glob_content`]-driven collections.
#[derive(Clone)]
pub struct Content<T>
where
    T: Send + Sync + 'static,
{
    /// Deserialized front matter, typically JSON or YAML.
    pub meta: T,
    /// The raw document body, stripped of metadata.
    pub text: String,
}

pub fn glob_content<G, T>(
    site_config: &mut SiteConfig<G>,
    path_base: &'static str,
    path_glob: &'static str,
    preload: fn(&str) -> Result<(T, String), anyhow::Error>,
) -> Handle<Vec<Content<T>>>
where
    G: Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    let task = FileLoaderTask::new(path_base, path_glob, move |_globals, file| {
        let text = String::from_utf8(file.metadata)?;
        let (meta, text) = preload(&text)?;
        Ok(Content { meta, text })
    });
    site_config.add_task_opaque(task)
}

/// Generate the functions used to initialize content files. These functions can
/// be used to parse the front matter using engines from crate `gray_matter`.
macro_rules! matter_parser {
    ($name:ident, $engine:path) => {
        #[doc = concat!(
            "This function can be used to extract metadata from a document with `D` as the frontmatter shape.\n",
            "Configured to use [`", stringify!($engine), "`] as the engine of the parser."
        )]
        pub fn $name<D>(content: &str) -> Result<(D, String), anyhow::Error>
        where
            D: for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
        {
            use gray_matter::{Matter, Pod};

            // We can cache the creation of the parser
            static PARSER: LazyLock<Matter<$engine>> = LazyLock::new(Matter::<$engine>::new);

            let entity = PARSER.parse(content)?;
            let object = entity
                .data
                .unwrap_or_else(Pod::new_hash)
                .deserialize::<D>()
                .map_err(|e| anyhow::anyhow!("Malformed frontmatter:\n{e}"))?;

            Ok((
                // Just the front matter
                object,
                // The rest of the content
                entity.content,
            ))
        }
    };
}

matter_parser!(yaml, YAML);
matter_parser!(json, JSON);
