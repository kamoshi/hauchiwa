#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod error;
mod gitmap;
mod io;
pub mod loader;
mod runtime;
#[cfg(feature = "reload")]
mod watch;

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Debug;
use std::fs;
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, IntoParallelRefMutIterator, ParallelIterator};

pub use crate::error::*;
pub use crate::gitmap::{GitInfo, GitRepo};
pub use crate::loader::Loader;
use crate::loader::{Loadable, LoaderOpts};
use crate::runtime::Tracker;
pub use crate::runtime::{Context, WithFile};

/// This value controls whether the library should run in the `Build` or the
/// `Watch` mode. In `Build` mode, the library builds every page of the website
/// just once and stops. In `Watch` mode, the library initializes the initial
/// state of the build process, opens up a websocket port, and watches for any
/// changes in the file system. Using the `Watch` mode allows you to enable
/// live-reload while editing the styles or the content of your website.
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    /// Run the library in `Build` mode.
    Build,
    /// Run the library in `Watch` mode.
    Watch,
}

/// `G` represents any additional data that should be globally available during
/// the HTML rendering process. If no such data is needed, it can be substituted
/// with `()`.
#[derive(Debug, Clone)]
pub struct Globals<G: Send + Sync = ()> {
    /// Generator mode.
    pub mode: Mode,
    /// Watch port
    pub port: Option<u16>,
    /// Any additional data.
    pub data: G,
}

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

fn init<G>(website: &mut Website<G>) -> Result<(), HauchiwaError>
where
    G: Send + Sync + 'static,
{
    crate::io::clear_dist()?;
    crate::io::clone_static()?;

    website.loaders_load()?;

    Ok(())
}

fn build<G>(website: &mut Website<G>, globals: &Globals<G>) -> Result<(), BuildError>
where
    G: Send + Sync + 'static,
{
    website.run_tasks(globals)?;

    let pages: Vec<_> = website
        .tasks
        .iter()
        .flat_map(|task| task.pages.iter())
        .collect();

    website
        .hooks
        .par_iter()
        .try_for_each(|hook| -> Result<_, BuildError> {
            match hook {
                Hook::PostBuild(callback) => callback(&pages).map_err(BuildError::Hook)?,
            };

            Ok(())
        })?;

    pages
        .par_iter()
        .try_for_each(|page| -> Result<_, BuildError> {
            let path = Utf8Path::new("dist").join(&page.path);

            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }

            let mut file = fs::File::create(&path)?;
            std::io::Write::write_all(&mut file, page.text.as_bytes())?;

            Ok(())
        })?;

    Ok(())
}

/// This struct represents the website which will be built by the generator. The individual
/// settings can be set by calling the `setup` function.
///
/// The `G` type parameter is the global data container accessible in every page renderer as `ctx.data`,
/// though it can be replaced with the `()` Unit if you don't need to pass any data.
pub struct Website<G: Send + Sync> {
    /// Preprocessors for files
    loaders: Vec<Box<dyn Loadable>>,
    /// Build tasks which can be used to generate pages.
    tasks: Vec<Task<G>>,
    /// Hooks
    hooks: Vec<Hook>,
}

impl<G: Send + Sync + 'static> Website<G> {
    pub fn config() -> Config<G> {
        Config::new()
    }

    pub fn build(&mut self, data: G) -> Result<(), HauchiwaError> {
        eprintln!(
            "Running {} in {} mode.",
            style("Hauchiwa").red(),
            style("build").blue()
        );

        let globals = Globals {
            mode: Mode::Build,
            port: None,
            data,
        };

        init(self)?;
        build(self, &globals)?;

        Ok(())
    }

    #[cfg(feature = "reload")]
    pub fn watch(&mut self, data: G) -> Result<(), HauchiwaError> {
        eprintln!(
            "Running {} in {} mode.",
            style("Hauchiwa").red(),
            style("watch").blue()
        );

        init(self)?;

        watch::watch(self, data)?;

        Ok(())
    }

    fn loaders_load(&mut self) -> Result<(), HauchiwaError> {
        let s = Instant::now();

        let len = self.loaders.len();
        let bar = ProgressBar::new(len as u64).with_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .expect("Error setting progress bar template")
                .progress_chars("#>-"),
        );

        let active = Arc::new(Mutex::new(HashSet::new()));

        self.loaders
            .par_iter_mut()
            .map(|loader| {
                let name = loader.name();

                {
                    let mut active = active.lock().unwrap();
                    active.insert(name.clone());
                    let msg = format_active(&active);
                    bar.set_message(msg);
                }

                let result = loader
                    .load()
                    .map_err(|err| HauchiwaError::Loader(name.to_string(), err));

                {
                    let mut active = active.lock().unwrap();
                    active.remove(&name);
                    let msg = format_active(&active);
                    bar.set_message(msg);
                    bar.inc(1);
                }

                result
            })
            .collect::<Result<Vec<_>, _>>()?;

        bar.finish_with_message(format!("Loaded assets {}", crate::io::as_overhead(s)));

        Ok(())
    }

    fn loaders_remove(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool {
        self.loaders
            .par_iter_mut()
            .any(|loader| loader.remove(obsolete))
    }

    fn loaders_reload(&mut self, modified: &HashSet<Utf8PathBuf>) -> Result<bool, LoaderError>
    where
        G: Send + Sync + 'static,
    {
        let len = self.loaders.len();
        let bar = ProgressBar::new(len as u64).with_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .expect("Error setting progress bar template")
                .progress_chars("#>-"),
        );

        let active = Arc::new(Mutex::new(HashSet::new()));

        let changed = self
            .loaders
            .par_iter_mut()
            .try_fold(
                || false,
                |acc, loader| -> Result<_, LoaderError> {
                    let name = loader.name();

                    {
                        let mut active = active.lock().unwrap();
                        active.insert(name.clone());
                        let msg = format_active(&active);
                        bar.set_message(msg);
                    }

                    let result = loader.reload(modified)?;

                    {
                        let mut active = active.lock().unwrap();
                        active.remove(&name);
                        let msg = format_active(&active);
                        bar.set_message(msg);
                        bar.inc(1);
                    }

                    Ok(acc || result)
                },
            )
            .try_reduce(|| false, |a, b| Ok(a || b))?;

        bar.finish_with_message("Reloaded assets");

        Ok(changed)
    }

    fn run_tasks(&mut self, globals: &Globals<G>) -> Result<(), BuildError> {
        let s = Instant::now();

        let items = self
            .loaders
            .iter()
            .flat_map(Loadable::items)
            .collect::<Vec<_>>();

        let total = self.tasks.len();
        let bar = ProgressBar::new(total as u64);
        bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .expect("invalid progress bar template")
                .progress_chars("#>-"),
        );

        let set = Arc::new(Mutex::new(HashSet::new()));

        self.tasks
            .par_iter_mut()
            .filter(|task| task.is_outdated(&items))
            .try_for_each(|task| -> Result<_, BuildError> {
                let name = Cow::from(task.name);

                {
                    let mut active = set.lock().unwrap();
                    active.insert(name.clone());
                    let msg = format_active(&active);
                    bar.set_message(msg);
                }

                task.run(globals, &items)
                    .map_err(|e| BuildError::Task(name.to_string(), e))?;

                {
                    let mut active = set.lock().unwrap();
                    active.remove(&name);
                    let msg = format_active(&active);
                    bar.set_message(msg);
                    bar.inc(1);
                }

                Ok(())
            })?;

        bar.finish_with_message(format!("Finished tasks {}", crate::io::as_overhead(s)));

        Ok(())
    }
}

fn format_active(active: &HashSet<Cow<str>>) -> String {
    const MAX: usize = 5;
    let mut names: Vec<_> = active.iter().cloned().collect();
    names.sort();

    if names.len() <= MAX {
        names.join(", ")
    } else {
        format!("{}… ({} total)", names[..MAX].join(", "), names.len())
    }
}

/// A builder struct for creating a `Website` with specified settings.
pub struct Config<G: Send + Sync> {
    loaders: Vec<Loader>,
    tasks: Vec<Task<G>>,
    hooks: Vec<Hook>,
    repo: Option<Arc<GitRepo>>,
}

impl<G: Send + Sync + 'static> Config<G> {
    fn new() -> Self {
        Self {
            loaders: Vec::default(),
            tasks: Vec::default(),
            hooks: Vec::new(),
            repo: None,
        }
    }

    /// Load git repository data from path.
    pub fn load_git(mut self, path: impl AsRef<Utf8Path>) -> anyhow::Result<Config<G>> {
        use crate::gitmap::{Options, map};
        let s = Instant::now();

        let data = map(Options {
            repository: path.as_ref().to_string(),
            revision: "HEAD".to_string(),
        })?;

        eprintln!("Loaded git repository data {}", crate::io::as_overhead(s));
        self.repo = Some(Arc::new(data));
        Ok(self)
    }

    pub fn add_loaders(mut self, processors: impl IntoIterator<Item = Loader>) -> Self {
        self.loaders.extend(processors);
        self
    }

    pub fn add_task(
        mut self,
        name: &'static str,
        task: fn(Context<G>) -> TaskResult<Vec<Page>>,
    ) -> Self {
        self.tasks.push(Task::new(name, task));
        self
    }

    pub fn finish(self) -> Website<G> {
        Website {
            loaders: self
                .loaders
                .into_iter()
                .map(|loader| {
                    loader.init(LoaderOpts {
                        repo: self.repo.clone(),
                    })
                })
                .collect(),
            tasks: self.tasks,
            hooks: self.hooks,
        }
    }

    pub fn add_hook(mut self, hook: Hook) -> Self {
        self.hooks.push(hook);
        self
    }
}

// ******************************
// *           Tasks            *
// ******************************

/// Represents a rendered output page, including its destination path,
/// content, and optional source file metadata.
///
/// This struct encapsulates the result of a build step (e.g., Markdown-to-HTML,
/// template rendering) and serves as the primary unit passed to hooks or written
/// to disk. If the page originates from a file, `from` retains its metadata.
pub struct Page {
    /// Relative output path where the page should be written.
    pub path: Utf8PathBuf,
    /// The full textual contents of the page (typically HTML).
    pub text: String,
    /// Optional source file metadata, if this page was generated from a file.
    pub from: Option<Arc<FileData>>,
}

impl Page {
    /// Creates a new `Page` with the given path and content, without linking to any source file.
    ///
    /// Use this for synthetic or programmatically generated pages.
    pub fn text(path: Utf8PathBuf, text: String) -> Self {
        Self {
            path,
            text,
            from: None,
        }
    }

    /// Creates a new `Page` with the given path, content, and originating file metadata.
    ///
    /// Use this when the page is derived from a file, such as a Markdown source,
    /// and you want to retain provenance information for tooling or debugging.
    pub fn text_with_file(path: Utf8PathBuf, text: String, from: Arc<FileData>) -> Self {
        Self {
            path,
            text,
            from: Some(from),
        }
    }
}

/// Result from a single executed task.
pub type TaskResult<T> = anyhow::Result<T, anyhow::Error>;

/// Task function pointer used to dynamically generate a website page. This
/// function is provided by the user from the userland, but it is used
/// internally during the build process.
type TaskFnPtr<D> = Arc<dyn Fn(Context<D>) -> TaskResult<Vec<Page>> + Send + Sync>;

/// Wraps `TaskFnPtr` and implements `Debug` trait for function pointer.
struct Task<D: Send + Sync> {
    pub name: &'static str,
    pub pages: Vec<Page>,
    init: bool,
    func: TaskFnPtr<D>,
    filters: Vec<Tracker>,
}

impl<D: Send + Sync> Task<D> {
    /// Create new task function pointer.
    fn new<F>(name: &'static str, func: F) -> Self
    where
        D: Send + Sync,
        F: Fn(Context<D>) -> TaskResult<Vec<Page>> + Send + Sync + 'static,
    {
        Self {
            name,
            pages: Default::default(),
            init: true,
            func: Arc::new(func),
            filters: Default::default(),
        }
    }

    fn is_outdated(&self, items: &[&Item]) -> bool {
        self.init || self.filters.iter().any(|f| f.check(items))
    }

    /// Run the task to generate a page.
    fn run(&mut self, globals: &Globals<D>, items: &[&Item]) -> anyhow::Result<()> {
        let tracker = Arc::new(RwLock::new(vec![]));
        let context = Context::new(globals, items, tracker.clone());

        self.pages = (self.func)(context)?;
        self.init = false;
        self.filters = Arc::into_inner(tracker).unwrap().into_inner().unwrap();

        Ok(())
    }
}

impl<G: Send + Sync> Debug for Task<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Task(*)")
    }
}

// ******************************
// *           Hooks            *
// ******************************

type HookCallback = Box<dyn Fn(&[&Page]) -> TaskResult<()> + Send + Sync>;

/// Represents a lifecycle hook invoked at specific points in the build pipeline.
///
/// Currently only supports a `PostBuild` phase, allowing users to register a
/// callback that runs after all pages have been generated. The hook receives
/// a reference to all rendered pages and may return a `TaskResult`, enabling
/// diagnostics, final transformations, or side-effects (e.g., search indexing,
/// sitemap generation, etc.).
pub enum Hook {
    PostBuild(HookCallback),
}

impl Hook {
    /// Creates a new `PostBuild` hook from the given callback.
    ///
    /// The provided function is invoked with all rendered pages, and may return
    /// an error to signal failure or abort subsequent tasks.
    ///
    /// Intended for use cases such as post-processing, validation, or export tasks
    /// that depend on the final structure of the site.
    pub fn post_build<F>(fun: F) -> Self
    where
        F: Fn(&[&Page]) -> TaskResult<()> + Send + Sync + 'static,
    {
        Hook::PostBuild(Box::new(fun))
    }
}

impl Debug for Hook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Hook::PostBuild(_) => write!(f, "Hook::PostBuild(*)"),
        }
    }
}

// ******************************
// *         Scheduler          *
// ******************************

type Dynamic = Arc<dyn Any + Send + Sync>;
type DynamicResult = Result<Dynamic, LazyAssetError>;

pub struct FileData {
    pub file: Utf8PathBuf,
    pub slug: Utf8PathBuf,
    pub area: Utf8PathBuf,
    pub info: Option<gitmap::GitInfo>,
}

struct FromFile {
    file: Arc<FileData>,
    /// Item computed on demand, cached in memory.
    data: LazyLock<DynamicResult, Box<dyn (FnOnce() -> DynamicResult) + Send + Sync>>,
}

struct Item {
    refl_type: TypeId,
    refl_name: &'static str,
    id: Box<str>,
    hash: Hash32,
    data: FromFile,
}
