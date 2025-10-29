use std::future::Future;

use crate::{
    loader::Runtime,
    task::Handle,
    SiteConfig,
};

pub fn async_asset<T, F, Fut>(
    site_config: &mut SiteConfig,
    async_closure: F,
) -> Handle<T>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(Runtime) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<T>> + Send + 'static,
{
    site_config.add_task((), move |_| {
        let rt = Runtime;
        let tokio_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to build runtime");
        tokio_rt.block_on(async_closure(rt)).unwrap()
    })
}
