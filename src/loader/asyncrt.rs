use std::{
    any::{TypeId, type_name},
    collections::HashSet,
    sync::{Arc, LazyLock},
};

use camino::Utf8PathBuf;

use crate::{
    FileData, FromFile, Item, Loader,
    loader::{Loadable, Runtime},
};

pub fn async_asset<T, F, Fut>(async_closure: F) -> Loader
where
    T: Send + Sync + 'static,
    F: Fn(Runtime) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<T>> + 'static,
{
    Loader::with(move |_| LoaderAsyncio::new(async_closure))
}

struct LoaderAsyncio<T, F, Fut>
where
    T: Send + Sync + 'static,
    F: Fn(Runtime) -> Fut + 'static,
    Fut: Future<Output = anyhow::Result<T>> + 'static,
{
    cached: Option<Item>,
    f1: F,
    rt: Runtime,
    tokio: tokio::runtime::Runtime,
}

impl<T, F, Fut> LoaderAsyncio<T, F, Fut>
where
    T: Send + Sync + 'static,
    F: Fn(Runtime) -> Fut + 'static,
    Fut: Future<Output = anyhow::Result<T>> + 'static,
{
    pub fn new(f1: F) -> Self {
        Self {
            cached: None,
            f1,
            rt: Runtime,
            tokio: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to build runtime"),
        }
    }
}

impl<T, F, Fut> Loadable for LoaderAsyncio<T, F, Fut>
where
    T: Send + Sync + 'static,
    F: Fn(Runtime) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<T>> + 'static,
{
    fn load(&mut self) {
        let f1 = &self.f1;

        let data = f1(self.rt.clone());
        let data = self.tokio.block_on(data).unwrap();

        self.cached = Some(Item {
            refl_type: TypeId::of::<T>(),
            refl_name: type_name::<T>(),
            // hash,
            data: FromFile {
                file: Arc::new(FileData {
                    file: "".into(),
                    slug: "".into(),
                    area: "".into(),
                    info: None,
                }),
                data: LazyLock::new(Box::new(move || Arc::new(data))),
            },
        });
    }

    fn reload(&mut self, _: &HashSet<Utf8PathBuf>) -> bool {
        false
    }

    fn items(&self) -> Vec<&crate::Item> {
        self.cached.iter().collect()
    }

    fn path_base(&self) -> &'static str {
        "./"
    }

    fn remove(&mut self, _: &HashSet<Utf8PathBuf>) -> bool {
        false
    }
}
