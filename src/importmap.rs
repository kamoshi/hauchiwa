use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// This matches the browser's Import Map specification.
/// <https://developer.mozilla.org/en-US/docs/Web/HTML/Element/script/type/importmap>
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImportMap {
    imports: BTreeMap<String, String>,
}

impl ImportMap {
    /// Creates a new, empty ImportMap
    pub fn new() -> Self {
        Self {
            imports: BTreeMap::new(),
        }
    }

    /// Register a new module key and its path.
    ///
    /// # Arguments
    /// * `key` - The module specifier (e.g., "svelte")
    /// * `value` - The URL or path (e.g., "/_app/svelte.js")
    pub fn register(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.imports.insert(key.into(), value.into());
        self
    }

    /// Serialize the map to a JSON string.
    pub fn to_json(&self) -> serde_json::Result<String> {
        // "pretty" is optional; strictly minified is fine too.
        serde_json::to_string(self)
    }

    /// Serialize the importmap to a proper HTML script tag importmap.
    pub fn to_html(&self) -> serde_json::Result<String> {
        self.to_json()
            .map(|json| format!(r#"<script type="importmap">{json}</script>"#))
    }
}

impl Default for ImportMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_importmap() {
        let mut map = ImportMap::new();
        map.register("svelte", "/_app/svelte.js");
        assert_eq!(
            map.to_html().unwrap(),
            r#"<script type="importmap">{"imports":{"svelte":"/_app/svelte.js"}}</script>"#
        );
    }

    #[test]
    fn test_default_importmap() {
        let map = ImportMap::default();
        assert!(map.imports.is_empty());
    }
}
