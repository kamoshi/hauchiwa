mod error;
mod gitmap;
mod loader;

use std::{any::Any, fmt::Debug, sync::Arc};

use camino::Utf8PathBuf;
use petgraph::{Graph, graph::NodeIndex};

/// 32 bytes length generic hash
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
struct Hash32([u8; 32]);

impl<T> From<T> for Hash32
where
    T: Into<[u8; 32]>,
{
    fn from(value: T) -> Self {
        Hash32(value.into())
    }
}

impl Hash32 {
    fn hash(buffer: impl AsRef<[u8]>) -> Self {
        blake3::Hasher::new()
            .update(buffer.as_ref())
            .finalize()
            .into()
    }

    fn hash_file(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        Ok(blake3::Hasher::new()
            .update_mmap_rayon(path)?
            .finalize()
            .into())
    }

    fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut acc = vec![0u8; 64];

        for (i, &byte) in self.0.iter().enumerate() {
            acc[i * 2] = HEX[(byte >> 4) as usize];
            acc[i * 2 + 1] = HEX[(byte & 0xF) as usize];
        }

        String::from_utf8(acc).unwrap()
    }
}

impl Debug for Hash32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hash32({})", self.to_hex())
    }
}

type Dynamic = Arc<dyn Any + Send + Sync>;
type DynamicResult = Result<Dynamic, error::LazyAssetError>;

#[derive(Debug)]
pub struct FileMetadata {
    pub file: Utf8PathBuf,
    pub area: Utf8PathBuf,
    pub info: Option<gitmap::GitInfo>,
}

struct Item {
    /// Type ID for the type contained by this item. This is how you can filter
    /// items without having to evalute and downcast lazy data.
    refl_type: TypeId,
    /// Type name just for diagnostics.
    refl_name: &'static str,
    /// Simple ID for the item, doesn't have to be unique. Either the file path
    /// or user provided static string, used for querying context.
    id: Box<str>,
    /// Hash for the file contents. In the case of assets loaded from multiple
    /// files, like bundled scripts or stylesheets this will be the hash of the
    /// entire bundle. It's used for checking which task needs to be redone.
    hash: Hash32,
    /// If the item can be traced back to a filesystem entry this will be filled.
    file: Option<Arc<FileMetadata>>,
    /// Item computed on demand, cached in memory.
    data: LazyLock<DynamicResult, Box<dyn (FnOnce() -> DynamicResult) + Send + Sync>>,
}

struct Task {
    dependencies: Vec<NodeIndex>,
}

pub struct Handle {
    pub(crate) index: NodeIndex,
}

/// A builder struct for creating a `Website` with specified settings.
pub struct SiteConfig {
    graph: Graph<Task, ()>,
}

impl SiteConfig {
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
        }
    }

    pub fn add_task(mut self, task: Task) -> Handle {
        let dependencies = task.dependencies.clone();
        let index = self.graph.add_node(task);

        for dependency in dependencies {
            self.graph.add_edge(index, dependency, ());
        }

        Handle { index }
    }
}

pub struct Site {
    graph: Graph<Task, ()>,
}

impl Site {
    pub fn new(config: SiteConfig) -> Self {
        Self {
            graph: config.graph,
        }
    }
}
