use std::sync::LazyLock;

use gray_matter::Matter;
use gray_matter::engine::{JSON, YAML};

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
			// We can cache the creation of the parser
			static PARSER: LazyLock<Matter<$engine>> = LazyLock::new(Matter::<$engine>::new);

			let entity = PARSER.parse(content);
            let object = entity
                .data
                .unwrap_or_else(|| gray_matter::Pod::new_array())
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
