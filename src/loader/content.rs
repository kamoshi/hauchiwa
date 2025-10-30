use crate::{
    SiteConfig,
    loader::{File, glob::GlobLoaderTask},
    task::Handle,
};
use camino::Utf8PathBuf;
use gray_matter::{Matter, engine::YAML};
use serde::{Deserialize, de::DeserializeOwned};
use std::{fs, marker::PhantomData};

#[derive(Deserialize)]
struct ContentData<T> {
    #[serde(default)]
    path: String,
    metadata: T,
    content: String,
}

#[derive(Clone)]
pub struct Content<T: Clone> {
    pub path: Utf8PathBuf,
    pub metadata: T,
    pub content: String,
}

pub fn glob_content<T: Clone, G>(
    site_config: &mut SiteConfig<G>,
    path_base: &'static str,
    path_glob: &'static str,
) -> Handle<Content<T>>
where
    T: DeserializeOwned + Send + Sync + 'static,
    G: Send + Sync + 'static,
{
    let task = GlobLoaderTask::new(path_base, path_glob, move |_, file: File<Vec<u8>>| {
        let data = std::str::from_utf8(&file.metadata)?;
        let matter = Matter::<YAML>::new();
        let parsed = matter.parse::<T>(data)?;
        Ok(Content {
            path: file.path,
            metadata: parsed.data.unwrap(),
            content: parsed.content,
        })
    });
    site_config.add_task_opaque(task)
}

#[derive(Clone, Default)]
pub struct Yaml<T>(PhantomData<T>);

impl<T: Default + 'static> Yaml<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

pub fn yaml<T: DeserializeOwned + Clone + Send + Sync + 'static, G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path: &'static str,
) -> Handle<T> {
    site_config.add_task((), move |_, _| -> T {
        let content = fs::read_to_string(path).unwrap();
        serde_yaml::from_str(&content).unwrap()
    })
}

#[derive(Clone, Default)]
pub struct Json<T>(PhantomData<T>);

impl<T: Default + 'static> Json<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

pub fn json<T: DeserializeOwned + Clone + Send + Sync + 'static, G: Send + Sync + 'static>(
    site_config: &mut SiteConfig<G>,
    path: &'static str,
) -> Handle<T> {
    site_config.add_task((), move |_, _| -> T {
        let content = fs::read_to_string(path).unwrap();
        serde_json::from_str(&content).unwrap()
    })
}
