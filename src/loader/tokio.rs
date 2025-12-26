use std::future::Future;

use crate::error::HauchiwaError;
use crate::{SiteConfig, task::Handle};

impl<G> SiteConfig<G>
where
    G: Send + Sync + 'static,
{
    pub fn load_async<R, F, Fut>(&mut self, callback: F) -> Result<Handle<R>, HauchiwaError>
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

        Ok(self.add_task((), move |_, _| executor.block_on(callback())))
    }
}
