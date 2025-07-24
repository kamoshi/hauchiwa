use crate::{Config, Context, Hook, Loader, Page, RuntimeError, Task};

pub struct PluginConfig<'a, G: Send + Sync> {
    pub(crate) config: &'a mut Config<G>,
}

impl<G: Send + Sync + 'static> PluginConfig<'_, G> {
    pub fn add_loaders(&mut self, processors: impl IntoIterator<Item = Loader>) -> &mut Self {
        self.config.loaders.extend(processors);
        self
    }

    pub fn add_task(
        &mut self,
        name: &'static str,
        task: fn(Context<G>) -> Result<Vec<Page>, RuntimeError>,
    ) -> &mut Self {
        self.config.tasks.push(Task::new(name, task));
        self
    }

    pub fn add_hook(&mut self, hook: Hook) -> &mut Self {
        self.config.hooks.push(hook);
        self
    }
}

pub struct Plugin<G: Send + Sync> {
    pub(crate) func: fn(&mut PluginConfig<G>) -> (),
}

impl<G: Send + Sync> Plugin<G> {
    pub const fn new(func: fn(&mut PluginConfig<G>) -> ()) -> Self {
        Self { func }
    }
}
