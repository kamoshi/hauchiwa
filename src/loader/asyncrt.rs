use std::{
    any::{TypeId, type_name},
    borrow::Cow,
    collections::HashSet,
    sync::{Arc, LazyLock},
};

use camino::Utf8PathBuf;

use crate::{
    Item, Loader, LoaderError,
    loader::{Loadable, Runtime},
};

/// Constructs a [`Loader`] from an async block that resolves to a typed asset.
///
/// This allows arbitrary
/// [async](https://doc.rust-lang.org/std/keyword.async.html) logic—such as
/// network fetches, API calls, or computational tasks to be embedded into the
/// build graph as a single item. The asset is constructed eagerly via a
/// single-threaded Tokio runtime.
///
/// ### Parameters
/// - `id`: Logical identifier for this asset (ideally unique).
/// - `async_closure`: An `async` function or block that returns a `T`.
///
/// ### Example
/// ```rust
/// use hauchiwa::{Context, TaskResult, Page, loader::async_asset};
///
/// struct Asset {
///     data: i32,
/// }
///
/// //loader
/// let loader = async_asset("async", async |rt| {
///     let data = async { 2 };
///     let data = data.await;;
///     Ok(Asset { data })
/// });
///
/// // task
/// fn task(ctx: Context) -> TaskResult<Vec<Page>> {
///     let Asset { data } = ctx.get::<Asset>("async")?;
///
///     Ok(vec![
///         Page::text("index.html".into(), format!("<h1>{data}</h1>"))
///     ])
/// }
/// ```
///
/// ### Notes
/// - This loader executes at build time; it is not reactive or incremental.
/// - If reproducibility is critical, ensure that `async_closure` is deterministic.
/// - Currently uses a local Tokio runtime per loader; avoid spawning nested tasks.
pub fn async_asset<T, F, Fut>(id: &'static str, async_closure: F) -> Loader
where
    T: Send + Sync + 'static,
    F: Fn(Runtime) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<T>> + 'static,
{
    Loader::with(move |_| LoaderAsyncio::new(id, async_closure))
}

struct LoaderAsyncio<T, F, Fut>
where
    T: Send + Sync + 'static,
    F: Fn(Runtime) -> Fut + 'static,
    Fut: Future<Output = anyhow::Result<T>> + 'static,
{
    id: &'static str,
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
    pub fn new(id: &'static str, f1: F) -> Self {
        Self {
            id,
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
    fn name(&self) -> Cow<'static, str> {
        self.id.into()
    }

    fn load(&mut self) -> Result<(), LoaderError> {
        let f1 = &self.f1;

        let data = f1(self.rt.clone());
        let data = self.tokio.block_on(data)?;

        self.cached = Some(Item {
            refl_type: TypeId::of::<T>(),
            refl_name: type_name::<T>(),
            id: self.id.into(),
            hash: Default::default(),
            data: LazyLock::new(Box::new(move || Ok(Arc::new(data)))),
            file: None,
        });

        Ok(())
    }

    fn reload(&mut self, _: &HashSet<Utf8PathBuf>) -> Result<bool, LoaderError> {
        Ok(false)
    }

    fn items(&self) -> Vec<&crate::Item> {
        self.cached.iter().collect()
    }

    fn path_base(&self) -> &'static str {
        "./styles"
    }

    fn remove(&mut self, _: &HashSet<Utf8PathBuf>) -> bool {
        false
    }
}
