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
use std::collections::HashSet;
use std::fmt::Debug;
use std::fs;
use std::sync::{Arc, LazyLock};
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use console::style;
use indicatif::{ParallelProgressIterator, ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, IntoParallelRefMutIterator, ParallelIterator};

pub use crate::error::*;
pub use crate::gitmap::{GitInfo, GitRepo};
pub use crate::loader::Loader;
use crate::loader::{Loadable, LoaderOpts};
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
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
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

    website.load_items()?;

    Ok(())
}

fn build<G>(website: &Website<G>, globals: &Globals<G>) -> Result<(), HauchiwaError>
where
    G: Send + Sync,
{
    let items = website.loaders.iter().flat_map(Loadable::items).collect();

    let total = website.tasks.len();
    let progress = ProgressBar::new(total as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )
            .expect("invalid progress bar template")
            .progress_chars("##-"),
    );

    let pages = website
        .tasks
        .par_iter()
        .map(|task| task.call(Context::new(globals, &items)))
        .progress_with(progress.clone())
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    progress.finish_with_message("Finished all tasks");

    let temp: Vec<_> = pages.iter().collect();
    for hook in &website.hooks {
        match hook {
            Hook::PostBuild(callback) => callback(&temp).unwrap(),
        }
    }

    // let builder = Arc::into_inner(builder).unwrap().into_inner().unwrap();

    for page in pages {
        let path = Utf8Path::new("dist").join(&page.path);

        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
        }
        let mut file = fs::File::create(&path)
            .map_err(|e| BuilderError::FileWriteError(path.to_owned(), e))?;
        std::io::Write::write_all(&mut file, page.text.as_bytes())
            .map_err(|e| BuilderError::FileWriteError(path.to_owned(), e))?;
    }

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

        watch::watch(self, data)?;

        Ok(())
    }

    fn load_items(&mut self) -> Result<(), HauchiwaError> {
        self.loaders
            .par_iter_mut()
            .map(|loader| loader.load())
            .collect::<Vec<_>>();

        Ok(())
    }

    fn remove_paths(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool {
        let mut changed = false;

        changed |= self
            .loaders
            .par_iter_mut()
            .any(|loader| loader.remove(obsolete));

        changed
    }

    fn reload_paths(&mut self, modified: &HashSet<Utf8PathBuf>) -> Result<bool, LoaderError>
    where
        G: Send + Sync + 'static,
    {
        let mut changed = false;

        changed |= self
            .loaders
            .par_iter_mut()
            .try_fold(
                || false,
                |acc, loader| -> Result<_, LoaderError> { Ok(acc || loader.reload(modified)?) },
            )
            .try_reduce(|| false, |a, b| Ok(a || b))?;

        Ok(changed)
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

    pub fn add_task(mut self, task: fn(Context<G>) -> TaskResult<Vec<Page>>) -> Self {
        self.tasks.push(Task::new(task));
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

pub struct Page {
    pub path: Utf8PathBuf,
    pub text: String,
    pub from: Option<Arc<FileData>>,
}

impl Page {
    pub fn text(path: Utf8PathBuf, text: String) -> Self {
        Self {
            path,
            text,
            from: None,
        }
    }

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
struct Task<D: Send + Sync>(TaskFnPtr<D>);

impl<D: Send + Sync> Task<D> {
    /// Create new task function pointer.
    fn new<F>(func: F) -> Self
    where
        D: Send + Sync,
        F: Fn(Context<D>) -> TaskResult<Vec<Page>> + Send + Sync + 'static,
    {
        Self(Arc::new(func))
    }

    /// Run the task to generate a page.
    fn call(&self, sack: Context<D>) -> TaskResult<Vec<Page>> {
        (self.0)(sack)
    }
}

impl<G: Send + Sync> Clone for Task<G> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
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

type HookCallback = Box<dyn Fn(&[&Page]) -> TaskResult<()>>;

pub enum Hook {
    PostBuild(HookCallback),
}

impl Hook {
    pub fn post_build<F>(fun: F) -> Self
    where
        F: Fn(&[&Page]) -> TaskResult<()> + 'static,
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
type DynamicResult = Result<Dynamic, ArcError>;

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
    // hash: Hash32,
    data: FromFile,
}
