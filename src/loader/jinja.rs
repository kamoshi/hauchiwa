use std::collections::HashSet;

use camino::{Utf8Path, Utf8PathBuf};
use glob::{Pattern, glob};
use petgraph::graph::NodeIndex;

use crate::Blueprint;
use crate::core::{Dynamic, Store, TaskContext};
use crate::engine::{TrackerState, Tracking, TypedCoarse};
use crate::error::HauchiwaError;

/// A loaded minijinja template environment.
///
/// This is the output of [`Blueprint::load_minijinja`]. It wraps a
/// [`minijinja::Environment`] with all matched template files pre-loaded.
///
/// Downstream tasks receive `&TemplateEnv` via their dependency handle
/// (`One<TemplateEnv>`). Any change to a watched template file causes
/// this loader to re-execute and all dependent tasks to re-run.
pub struct TemplateEnv(minijinja::Environment<'static>);

impl TemplateEnv {
    /// Returns a reference to the underlying minijinja environment.
    pub fn env(&self) -> &minijinja::Environment<'static> {
        &self.0
    }

    /// Retrieves a compiled template by name.
    pub fn get_template(
        &self,
        name: &str,
    ) -> Result<minijinja::Template<'_, '_>, minijinja::Error> {
        self.0.get_template(name)
    }
}

pub(crate) struct GlobJinja {
    glob_entry: Vec<String>,
    glob_watch: Vec<Pattern>,
    offset: Option<String>,
}

impl GlobJinja {
    pub fn new(
        glob_entry: Vec<String>,
        glob_watch: Vec<String>,
        offset: Option<String>,
    ) -> Result<Self, HauchiwaError> {
        Ok(Self {
            glob_entry,
            glob_watch: glob_watch
                .iter()
                .map(|p| Pattern::new(p))
                .collect::<Result<_, _>>()?,
            offset,
        })
    }
}

impl<G> TypedCoarse<G> for GlobJinja
where
    G: Send + Sync + 'static,
{
    type Output = TemplateEnv;

    fn get_name(&self) -> String {
        self.glob_entry.join(", ")
    }

    fn dependencies(&self) -> Vec<NodeIndex> {
        vec![]
    }

    fn get_watched(&self) -> Vec<Utf8PathBuf> {
        self.glob_watch
            .iter()
            .map(|pat| Utf8PathBuf::from(pat.as_str()))
            .collect()
    }

    fn execute(
        &self,
        _: &TaskContext<G>,
        _: &mut Store,
        _: &[Dynamic],
    ) -> anyhow::Result<(Tracking, Self::Output)> {
        let mut env = minijinja::Environment::new();

        for glob_entry in &self.glob_entry {
            for path in glob(glob_entry)? {
                let path = Utf8PathBuf::try_from(path?)?;
                let source = std::fs::read_to_string(&path)?;

                let name = match &self.offset {
                    Some(offset) => path.strip_prefix(offset).unwrap_or(&path).to_string(),
                    None => path.to_string(),
                };

                env.add_template_owned(name, source)
                    .map_err(|e| anyhow::anyhow!("Failed to load template {path}: {e}"))?;
            }
        }

        Ok((Tracking::default(), TemplateEnv(env)))
    }

    fn is_dirty(&self, path: &Utf8Path) -> bool {
        self.glob_watch.iter().any(|p| p.matches(path.as_str()))
    }

    fn is_valid(
        &self,
        _: &[Option<TrackerState>],
        _: &[Dynamic],
        _: &HashSet<NodeIndex>,
    ) -> bool {
        // No upstream dependencies — always valid unless explicitly dirtied by a file change.
        true
    }
}

/// A builder for configuring the minijinja template loader.
pub struct JinjaLoader<'a, G>
where
    G: Send + Sync,
{
    blueprint: &'a mut Blueprint<G>,
    globs: Vec<String>,
    root: Option<String>,
}

impl<'a, G> JinjaLoader<'a, G>
where
    G: Send + Sync + 'static,
{
    pub(crate) fn new(blueprint: &'a mut Blueprint<G>) -> Self {
        Self {
            blueprint,
            globs: Vec::new(),
            root: None,
        }
    }

    /// Add a glob pattern to find template files.
    pub fn glob(mut self, glob: impl Into<String>) -> Self {
        self.globs.push(glob.into());
        self
    }

    /// Strip this prefix from file paths when computing template names.
    ///
    /// For example, with `.root("templates")`, a file at
    /// `templates/layouts/base.html` will be registered as `layouts/base.html`.
    /// Without a root, the full relative path is used as the template name.
    pub fn root(mut self, root: impl Into<String>) -> Self {
        self.root = Some(root.into());
        self
    }

    /// Register the task with the blueprint.
    pub fn register(self) -> Result<crate::One<TemplateEnv>, HauchiwaError> {
        let task = GlobJinja::new(self.globs.clone(), self.globs, self.root)?;
        Ok(self.blueprint.add_task_coarse(task))
    }
}

impl<G> Blueprint<G>
where
    G: Send + Sync + 'static,
{
    /// Starts configuring a minijinja template loader.
    ///
    /// Loads all files matching the given glob patterns into a
    /// [`minijinja::Environment`] and returns a [`One<TemplateEnv>`] handle.
    /// In watch mode, any change to a matched file re-executes this loader and
    /// invalidates all downstream tasks.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # let mut config = hauchiwa::Blueprint::<()>::new();
    /// let templates = config
    ///     .load_minijinja()
    ///     .glob("templates/**/*.html")
    ///     .root("templates")
    ///     .register()
    ///     .unwrap();
    /// ```
    pub fn load_minijinja(&mut self) -> JinjaLoader<'_, G> {
        JinjaLoader::new(self)
    }
}
