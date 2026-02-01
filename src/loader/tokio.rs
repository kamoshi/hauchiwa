use std::future::Future;

use crate::Blueprint;
use crate::TaskContext;
use crate::engine::HandleC;
use crate::error::HauchiwaError;

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Executes an asynchronous closure within a temporary Tokio runtime.
    ///
    /// This loader is useful for running asynchronous tasks that are not
    /// natively supported by the synchronous build graph. It spawns a new
    /// single-threaded Tokio runtime to block on the provided future.
    ///
    /// # Generics
    ///
    /// * `R`: The return type of the future.
    /// * `F`: The type of the closure that returns the future.
    /// * `Fut`: The type of the [`Future`] returned by the closure.
    ///
    /// # Arguments
    ///
    /// * `callback`: A closure that takes no arguments and returns a [`Future`]
    ///   resolving to an [`anyhow::Result<R>`].
    ///
    /// # Returns
    ///
    /// A [`Handle`] to the result `R` in the build graph.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let data = config.load_async(|| async {
    ///     let response = reqwest::get("https://example.com/data.json").await?;
    ///     let json: serde_json::Value = response.json().await?;
    ///     Ok(json)
    /// })?;
    /// ```
    pub fn load_async<R, F, Fut>(&mut self, callback: F) -> Result<HandleC<R>, HauchiwaError>
    where
        G: Send + Sync + 'static,
        R: Send + Sync + 'static,
        F: Fn(&TaskContext<G>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<R>> + Send + 'static,
    {
        let executor = Box::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
        );

        let handle = self.task().run(move |ctx| executor.block_on(callback(ctx)));

        Ok(handle)
    }
}
