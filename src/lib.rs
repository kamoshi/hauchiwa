#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod error;
mod gitmap;
mod io;
mod loader;
pub mod md;
pub mod plugin;
mod runtime;
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
use sha2::{Digest, Sha256};
// use sitemap_rs::url::{ChangeFrequency, Url};
// use sitemap_rs::url_set::UrlSet;

pub use crate::error::*;
pub use crate::gitmap::{GitInfo, GitRepo};
pub use crate::loader::assets::Assets;
pub use crate::loader::content::Content;
pub use crate::runtime::{Context, ViewPage};

type ArcAny = Arc<dyn Any + Send + Sync>;

pub struct Script {
    pub path: Utf8PathBuf,
}

pub struct Stylesheet {
    pub path: Utf8PathBuf,
}

pub struct Svelte {
    pub html: String,
    pub init: Utf8PathBuf,
}

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
        Sha256::digest(buffer).into()
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

fn load_repo() -> GitRepo {
    let s = Instant::now();

    let repo = crate::gitmap::map(crate::gitmap::Options {
        repository: ".".to_string(),
        revision: "HEAD".to_string(),
    })
    .unwrap();

    eprintln!("Loaded git repository data {}", crate::io::as_overhead(s));

    repo
}

fn init<G>(website: &mut Website<G>) -> Result<(), HauchiwaError>
where
    G: Send + Sync + 'static,
{
    crate::io::clear_dist()?;
    crate::io::clone_static()?;

    let repo = load_repo();

    website.load_items(&repo)?;

    Ok(())
}

fn build<G>(website: &Website<G>, globals: &Globals<G>) -> Result<(), HauchiwaError>
where
    G: Send + Sync,
{
    let items = []
        .into_iter()
        .chain(
            website
                .loaders_content
                .iter()
                .flat_map(|loader| loader.items()),
        )
        .chain(
            website
                .loaders_assets
                .iter()
                .flat_map(|loader| loader.items()),
        )
        .collect::<Vec<_>>();

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

    for (path, data) in pages {
        let path = Utf8Path::new("dist").join(path);

        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
        }
        let mut file = fs::File::create(&path)
            .map_err(|e| BuilderError::FileWriteError(path.to_owned(), e))?;
        std::io::Write::write_all(&mut file, data.as_bytes())
            .map_err(|e| BuilderError::FileWriteError(path.to_owned(), e))?;
    }

    Ok(())
}

// ******************************
// *    Website Configuration   *
// ******************************

/// This struct represents the website which will be built by the generator. The individual
/// settings can be set by calling the `setup` function.
///
/// The `G` type parameter is the global data container accessible in every page renderer as `ctx.data`,
/// though it can be replaced with the `()` Unit if you don't need to pass any data.
pub struct Website<G: Send + Sync> {
    /// Rendered assets and content are outputted to this directory.
    /// All collections added to this website.
    loaders_content: Vec<Content>,
    /// Preprocessors for files
    loaders_assets: Vec<Assets>,
    /// Build tasks which can be used to generate pages.
    tasks: Vec<Task<G>>,
    /// Sitemap options
    opts_sitemap: Option<Utf8PathBuf>,
    /// Hooks
    hooks: Vec<Hook>,
}

impl<G: Send + Sync + 'static> Website<G> {
    pub fn configure() -> WebsiteConfiguration<G> {
        WebsiteConfiguration::new()
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

    pub fn watch(&mut self, data: G) -> Result<(), HauchiwaError> {
        eprintln!(
            "Running {} in {} mode.",
            style("Hauchiwa").red(),
            style("watch").blue()
        );

        watch::watch(self, data)?;

        Ok(())
    }

    fn load_items(&mut self, repo: &GitRepo) -> Result<(), HauchiwaError> {
        self.loaders_content
            .par_iter_mut()
            .map(|loader| loader.load(repo))
            .collect::<Result<Vec<_>, _>>()?;

        self.loaders_assets
            .par_iter_mut()
            .map(|loader| loader.load())
            .collect::<Vec<_>>();

        Ok(())
    }

    fn remove_paths(&mut self, obsolete: &HashSet<Utf8PathBuf>) -> bool {
        let mut changed = false;

        changed |= self
            .loaders_content
            .par_iter_mut()
            .any(|loader| loader.remove(obsolete));

        changed |= self
            .loaders_assets
            .par_iter_mut()
            .any(|loader| loader.remove(obsolete));

        changed
    }

    fn reload_paths(&mut self, modified: &HashSet<Utf8PathBuf>, repo: &GitRepo) -> bool
    where
        G: Send + Sync + 'static,
    {
        let mut changed = false;

        changed |= self
            .loaders_content
            .par_iter_mut()
            .any(|loader| loader.reload(modified, repo).unwrap());

        changed |= self
            .loaders_assets
            .par_iter_mut()
            .any(|loader| loader.reload(modified));

        changed
    }
}

// fn build_hooks<G>(website: &Website<G>, scheduler: &Scheduler<G>) -> Result<(), HookError>
// where
//     G: Send + Sync,
// {
//     let s = Instant::now();
//     for hook in &website.hooks {
//         let pages: Vec<_> = scheduler
//             .tracked
//             .iter()
//             .flat_map(|trace| &trace.path)
//             .collect();

//         match hook {
//             Hook::PostBuild(fun) => fun(&pages)?,
//         }
//     }

//     eprintln!("Ran user hooks {}", crate::io::as_overhead(s));

//     Ok(())
// }

// fn build_sitemap<G>(website: &Website<G>, scheduler: &Scheduler<G>) -> Result<(), SitemapError>
// where
//     G: Send + Sync,
// {
//     if let Some(ref opts) = website.opts_sitemap {
//         let sitemap = scheduler.build_sitemap(opts);
//         fs::write("dist/sitemap.xml", sitemap)?;
//     }
//     Ok(())
// }

/// A builder struct for creating a `Website` with specified settings.
pub struct WebsiteConfiguration<G: Send + Sync> {
    loaders_content: Vec<Content>,
    loaders_assets: Vec<Assets>,
    tasks: Vec<Task<G>>,
    opts_sitemap: Option<Utf8PathBuf>,
    hooks: Vec<Hook>,
}

impl<G: Send + Sync + 'static> WebsiteConfiguration<G> {
    fn new() -> Self {
        Self {
            loaders_content: Vec::default(),
            loaders_assets: Vec::default(),
            tasks: Vec::default(),
            opts_sitemap: None,
            hooks: Vec::new(),
        }
    }

    pub fn add_content(mut self, collections: impl IntoIterator<Item = Content>) -> Self {
        self.loaders_content.extend(collections);
        self
    }

    pub fn add_assets(mut self, processors: impl IntoIterator<Item = Assets>) -> Self {
        self.loaders_assets.extend(processors);
        self
    }

    pub fn add_task(mut self, task: fn(Context<G>) -> TaskResult<TaskPaths>) -> Self {
        self.tasks.push(Task::new(task));
        self
    }

    pub fn set_opts_sitemap(mut self, path: impl AsRef<str>) -> Self {
        self.opts_sitemap = Some(path.as_ref().into());
        self
    }

    pub fn finish(self) -> Website<G> {
        Website {
            loaders_content: self.loaders_content,
            loaders_assets: self.loaders_assets,
            tasks: self.tasks,
            opts_sitemap: self.opts_sitemap,
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

/// Rendered content from the userland.
type TaskPaths = Vec<(Utf8PathBuf, String)>;

/// Result from a single executed task.
pub type TaskResult<T> = anyhow::Result<T, anyhow::Error>;

/// Task function pointer used to dynamically generate a website page. This
/// function is provided by the user from the userland, but it is used
/// internally during the build process.
type TaskFnPtr<D> = Arc<dyn Fn(Context<D>) -> TaskResult<TaskPaths> + Send + Sync>;

/// Wraps `TaskFnPtr` and implements `Debug` trait for function pointer.
struct Task<D: Send + Sync>(TaskFnPtr<D>);

impl<D: Send + Sync> Task<D> {
    /// Create new task function pointer.
    fn new<F>(func: F) -> Self
    where
        D: Send + Sync,
        F: Fn(Context<D>) -> TaskResult<TaskPaths> + Send + Sync + 'static,
    {
        Self(Arc::new(func))
    }

    /// Run the task to generate a page.
    fn call(&self, sack: Context<D>) -> TaskResult<TaskPaths> {
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

pub enum Hook {
    PostBuild(Box<dyn Fn(&[&(Utf8PathBuf, String)]) -> TaskResult<()>>),
}

impl Hook {
    pub fn post_build<F>(fun: F) -> Self
    where
        F: Fn(&[&(Utf8PathBuf, String)]) -> TaskResult<()> + 'static,
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

#[derive(Debug)]
struct InputContent {
    area: Utf8PathBuf,
    meta: Dynamic,
    text: String,
}

enum Input {
    Content(InputContent),
    /// Just the item, stored in memory, readily accessible.
    Just(Dynamic),
    /// Item computed on demand, cached in memory.
    Lazy(LazyLock<Dynamic, Box<dyn (FnOnce() -> Dynamic) + Send + Sync>>),
}

struct InputItem {
    refl_type: TypeId,
    refl_name: &'static str,
    hash: Hash32,
    file: Utf8PathBuf,
    slug: Utf8PathBuf,
    data: Input,
    info: Option<gitmap::GitInfo>,
}

// pub fn build_sitemap(&self, opts: &Utf8Path) -> Vec<u8> {
//     let urls = self
//         .tracked
//         .iter()
//         .flat_map(|x| &x.path)
//         .collect::<HashSet<_>>()
//         .into_iter()
//         .map(|path| {
//             Url::builder(opts.join(&path.0).parent().unwrap().to_string())
//                 .change_frequency(ChangeFrequency::Monthly)
//                 .priority(0.8)
//                 .build()
//                 .expect("failed a <url> validation")
//         })
//         .collect::<Vec<_>>();
//     let urls = UrlSet::new(urls).expect("failed a <urlset> validation");
//     let mut buf = Vec::<u8>::new();
//     urls.write(&mut buf).expect("failed to write XML");
//     buf
// }
