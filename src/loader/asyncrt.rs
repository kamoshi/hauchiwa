use std::future::Future;

use crate::error::HauchiwaError;
use crate::{SiteConfig, loader::Runtime, task::Handle};

pub fn async_asset<G, R, F, Fut>(
    config: &mut SiteConfig<G>,
    callback: F,
) -> Result<Handle<R>, HauchiwaError>
where
    G: Send + Sync + 'static,
    R: Send + Sync + 'static,
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<R>> + Send + 'static,
{
    let executor = Box::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?,
    );

    Ok(config.add_task((), move |_, _| executor.block_on(callback())))
}
