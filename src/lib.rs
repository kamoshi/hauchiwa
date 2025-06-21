#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod error;
mod gitmap;
mod io;
pub mod md;
mod watch;

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use std::{fs, mem};

use camino::{Utf8Path, Utf8PathBuf};
use console::style;
use indicatif::{ParallelProgressIterator, ProgressBar, ProgressIterator, ProgressStyle};
use rayon::iter::{
    IntoParallelIterator, IntoParallelRefIterator, ParallelExtend, ParallelIterator,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sitemap_rs::url::{ChangeFrequency, Url};
use sitemap_rs::url_set::UrlSet;

pub use crate::error::*;
pub use crate::gitmap::{GitInfo, GitRepo};

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
    /// Any additional data.
    pub data: G,
}

/// 32 bytes length generic hash
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

// ******************************
// *    Website Configuration   *
// ******************************

/// This struct represents the website which will be built by the generator. The individual
/// settings can be set by calling the `setup` function.
///
/// The `G` type parameter is the global data container accessible in every page renderer as `ctx.data`,
/// though it can be replaced with the `()` Unit if you don't need to pass any data.
#[derive(Debug)]
pub struct Website<G: Send + Sync> {
    /// Rendered assets and content are outputted to this directory.
    /// All collections added to this website.
    loaders_content: Vec<Content>,
    /// Preprocessors for files
    loaders_assets: Vec<Assets>,
    /// Build tasks which can be used to generate pages.
    tasks: Vec<Task<G>>,
    /// Global scripts
    global_scripts: HashMap<&'static str, &'static str>,
    /// Global styles
    global_styles: Vec<Utf8PathBuf>,
    /// Sitemap options
    opts_sitemap: Option<Utf8PathBuf>,
    /// Hooks
    hooks: Vec<Hook>,
}

impl<G: Send + Sync + 'static> Website<G> {
    pub fn configure() -> WebsiteConfiguration<G> {
        WebsiteConfiguration::new()
    }

    pub fn build(&self, data: G) -> Result<(), HauchiwaError> {
        eprintln!(
            "Running {} in {} mode.",
            style("Hauchiwa").red(),
            style("build").blue()
        );

        let context = Globals {
            mode: Mode::Build,
            data,
        };

        let _ = self.init(&context)?;

        Ok(())
    }

    pub fn watch(&self, data: G) -> Result<(), HauchiwaError> {
        eprintln!(
            "Running {} in {} mode.",
            style("Hauchiwa").red(),
            style("watch").blue()
        );

        let context = Globals {
            mode: Mode::Watch,
            data,
        };

        self.init(&context)?.watch(self)?;

        Ok(())
    }

    fn init<'a>(&'a self, context: &'a Globals<G>) -> Result<Scheduler<'a, G>, HauchiwaError> {
        let s = Instant::now();

        crate::io::clear_dist()?;
        crate::io::clone_static()?;

        let repo = {
            let s = Instant::now();

            let repo = crate::gitmap::map(crate::gitmap::Options {
                repository: ".".to_string(),
                revision: "HEAD".to_string(),
            })
            .unwrap();

            eprintln!("Loaded git repository data {}", crate::io::as_overhead(s));

            repo
        };

        let mut items = vec![];
        items.extend(css_load_paths(&self.global_styles)?);
        items.extend(load_scripts(&self.global_scripts));

        self.loaders_content
            .par_iter()
            .map(|loader| loader.load(&repo))
            .collect::<Result<Vec<_>, _>>()?;

        items.extend(self.loaders_content.iter().flat_map(|xd| {
            xd.cached
                .lock()
                .unwrap()
                .values()
                .cloned()
                .collect::<Vec<_>>()
        }));

        items.par_extend(
            self.loaders_assets
                .par_iter()
                .flat_map(|loader| loader.load()),
        );

        let mut scheduler = Scheduler::new(self, context, items);
        scheduler.refresh()?;
        scheduler.fulfill_build_requests()?;

        build_hooks(self, &scheduler)?;
        build_sitemap(self, &scheduler)?;

        eprintln!(
            "Successfully built website in {}",
            format!("{}ms", Instant::now().duration_since(s).as_millis())
        );

        Ok(scheduler)
    }

    /// Load items by a set of paths.
    fn load_set(&self, paths: &HashSet<Utf8PathBuf>, repo: &GitRepo) -> Result<(), LoaderError> {
        for path in paths {
            for collection in &self.loaders_content {
                collection.load_once(repo, path)?
            }
        }

        Ok(())
    }
}

fn build_hooks<G>(website: &Website<G>, scheduler: &Scheduler<G>) -> Result<(), HookError>
where
    G: Send + Sync,
{
    let s = Instant::now();
    for hook in &website.hooks {
        let pages: Vec<_> = scheduler
            .tracked
            .iter()
            .flat_map(|trace| &trace.path)
            .collect();

        match hook {
            Hook::PostBuild(fun) => fun(&pages)?,
        }
    }

    eprintln!("Ran user hooks {}", crate::io::as_overhead(s));

    Ok(())
}

fn build_sitemap<G>(website: &Website<G>, scheduler: &Scheduler<G>) -> Result<(), SitemapError>
where
    G: Send + Sync,
{
    if let Some(ref opts) = website.opts_sitemap {
        let sitemap = scheduler.build_sitemap(opts);
        fs::write("dist/sitemap.xml", sitemap)?;
    }
    Ok(())
}

fn css_load_paths(paths: &[Utf8PathBuf]) -> Result<Vec<InputItem>, StylesheetError> {
    let s = Instant::now();
    let mut items = Vec::new();

    for path in paths {
        let pattern = path.join("**/[!_]*.scss");
        let results = glob::glob(pattern.as_str())?;

        for path in results {
            let item = css_compile(path?)?;
            items.push(item);
        }
    }

    eprintln!(
        "Loaded global CSS stylesheets! {}",
        crate::io::as_overhead(s)
    );

    Ok(items)
}

fn css_compile(file: PathBuf) -> Result<InputItem, StylesheetError> {
    let opts = grass::Options::default().style(grass::OutputStyle::Compressed);

    let file =
        Utf8PathBuf::try_from(file).map_err(|e| StylesheetError::InvalidFileName(e.to_string()))?;
    let stylesheet =
        grass::from_path(&file, &opts).map_err(|e| StylesheetError::Compiler(e.to_string()))?;

    Ok(InputItem {
        hash: Hash32::hash(&stylesheet),
        file: file.clone(),
        slug: file,
        data: Input::Stylesheet(InputStylesheet { stylesheet }),
        info: None,
    })
}

fn load_scripts(entrypoints: &HashMap<&str, &str>) -> Vec<InputItem> {
    let mut cmd = Command::new("esbuild");

    for (alias, path) in entrypoints.iter() {
        cmd.arg(format!("{}={}", alias, path));
    }

    let path_scripts = Utf8Path::new(".cache/scripts/");

    let s = Instant::now();
    let _ = cmd
        .arg("--format=esm")
        .arg("--bundle")
        .arg("--minify")
        .arg(format!("--outdir={}", path_scripts))
        .output()
        .unwrap();

    eprintln!("Loaded global JS scripts! {}", crate::io::as_overhead(s));

    entrypoints
        .keys()
        .map(|key| {
            let file = path_scripts.join(key).with_extension("js");
            let buffer = fs::read(&file).unwrap();
            let hash = Hash32::hash(buffer);

            InputItem {
                slug: file.clone(),
                file,
                hash,
                data: Input::Script,
                info: None,
            }
        })
        .collect()
}

/// A builder struct for creating a `Website` with specified settings.
#[derive(Debug)]
pub struct WebsiteConfiguration<G: Send + Sync> {
    loaders_content: Vec<Content>,
    loaders_assets: Vec<Assets>,
    tasks: Vec<Task<G>>,
    global_scripts: HashMap<&'static str, &'static str>,
    global_styles: Vec<Utf8PathBuf>,
    opts_sitemap: Option<Utf8PathBuf>,
    hooks: Vec<Hook>,
}

impl<G: Send + Sync + 'static> WebsiteConfiguration<G> {
    fn new() -> Self {
        Self {
            loaders_content: Vec::default(),
            loaders_assets: Vec::default(),
            tasks: Vec::default(),
            global_scripts: HashMap::default(),
            global_styles: Vec::default(),
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

    pub fn add_scripts(
        mut self,
        scripts: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) -> Self {
        self.global_scripts.extend(scripts);
        self
    }

    pub fn add_styles(mut self, paths: impl IntoIterator<Item = Utf8PathBuf>) -> Self {
        self.global_styles.extend(paths);
        self
    }

    pub fn add_task(mut self, fun: fn(Context<G>) -> TaskResult<TaskPaths>) -> Self {
        self.tasks.push(Task::new(fun));
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
            global_scripts: self.global_scripts,
            global_styles: self.global_styles,
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
// *       Content Loader       *
// ******************************

type ArcAny = Arc<dyn Any + Send + Sync>;

type ContentFnPtr =
    Arc<dyn Fn(&str) -> Result<(ArcAny, String), LoaderFileCallbackError> + Send + Sync>;

#[derive(Clone)]
struct ContentFn(ContentFnPtr);

impl ContentFn {
    fn new<D>(parse_matter: fn(&str) -> Result<(D, String), anyhow::Error>) -> Self
    where
        D: for<'de> Deserialize<'de> + Send + Sync + 'static,
    {
        Self(Arc::new(move |content| {
            let (meta, data) = parse_matter(content).map_err(LoaderFileCallbackError)?;
            Ok((Arc::new(meta), data))
        }))
    }

    fn call(&self, data: &str) -> Result<(ArcAny, String), LoaderFileCallbackError> {
        (self.0)(data)
    }
}

impl Debug for ContentFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InitFn(*)")
    }
}

/// How to load data?
#[derive(Debug)]
enum ContentStrategy {
    Glob(LoaderGlob),
}

#[derive(Debug)]
struct LoaderGlob {
    base: &'static str,
    glob: &'static str,
    init: ContentFn,
}

impl LoaderGlob {
    /// Read many files into InputItem
    fn read(&self, repo: &GitRepo) -> Result<Vec<InputItem>, LoaderError> {
        let pattern = Utf8Path::new(self.base).join(self.glob);
        let mut vec = vec![];

        for path in glob::glob(pattern.as_str())? {
            let path = Utf8PathBuf::try_from(path?)?;
            if let Some(item) = self
                .read_file(path.clone(), repo)
                .map_err(|e| LoaderError::LoaderGlobFile(path, e))?
            {
                vec.push(item);
            }
        }

        Ok(vec)
    }

    /// Read single file into InputItem
    fn read_once(&self, path: &Utf8Path, repo: &GitRepo) -> Result<Option<InputItem>, LoaderError> {
        let pattern = Utf8Path::new(self.base).join(self.glob);
        let pattern = glob::Pattern::new(pattern.as_str())?;

        if !pattern.matches_path(path.as_std_path()) {
            return Ok(None);
        };

        let path = path.to_owned();
        let item = self
            .read_file(path.clone(), repo)
            .map_err(|e| LoaderError::LoaderGlobFile(path, e))?;

        Ok(item)
    }

    /// Helper function, convert file into InputItem
    /// TODO: based on loader cache, here we can use Hash32 to check if the
    /// previously loaded content item already exists, and *if* we have it, we
    /// can skip the `init.call`, because we can just reuse the old one.
    fn read_file(
        &self,
        path: Utf8PathBuf,
        repo: &GitRepo,
    ) -> Result<Option<InputItem>, LoaderFileError> {
        if path.is_dir() {
            return Ok(None);
        }

        let bytes = fs::read(&path)?;
        let hash = Hash32::hash(&bytes);
        let text = String::from_utf8_lossy(&bytes);
        let (meta, text) = self.init.call(&text)?;

        let area = match path.file_stem() {
            Some("index") => path
                .parent()
                .map(ToOwned::to_owned)
                .unwrap_or(path.with_extension("")),
            _ => path.with_extension(""),
        };

        let slug = area.strip_prefix(self.base).unwrap_or(&path).to_owned();

        Ok(Some(InputItem {
            hash,
            info: repo.files.get(path.as_str()).cloned(),
            file: path,
            slug,
            data: Input::Content(InputContent { area, meta, text }),
        }))
    }
}

/// An opaque representation of a source of inputs loaded into the generator.
/// You can think of a single collection as a set of written articles with
/// shared frontmatter shape, for example your blog posts.
///
/// Hovewer, a collection can also load additional files like images or custom
/// assets. This is useful when you want to colocate assets and images next to
/// the articles. A common use case is to directly reference the images relative
/// to the markdown file.
#[derive(Debug)]
pub struct Content {
    /// Data loading strategy
    loader: ContentStrategy,
    /// Content loaded and saved between multiple loads.
    cached: Mutex<HashMap<String, InputItem>>,
}

impl Content {
    /// Create a new collection which draws content from the filesystem files
    /// via a glob pattern. Usually used to collect articles written as markdown
    /// files, however it is completely format agnostic.
    ///
    /// The parameter `parse_matter` allows you to customize how the metadata
    /// should be parsed. Default functions for the most common formats are
    /// provided by library:
    /// * [`parse_matter_json`](`crate::parse_matter_json`) - parse JSON metadata
    /// * [`parse_matter_yaml`](`crate::parse_matter_yaml`) - parse YAML metadata
    ///
    /// # Examples
    ///
    /// ```rust
    /// Collection::glob_with("content", "posts/**/*", ["md"], parse_matter_yaml::<Post>);
    /// ```
    pub fn glob<D>(
        path_base: &'static str,
        path_glob: &'static str,
        parse_matter: fn(&str) -> Result<(D, String), anyhow::Error>,
    ) -> Self
    where
        D: for<'de> Deserialize<'de> + Send + Sync + 'static,
    {
        Self {
            loader: ContentStrategy::Glob(LoaderGlob {
                base: path_base,
                glob: path_glob,
                init: ContentFn::new(parse_matter),
            }),
            cached: Mutex::new(HashMap::new()),
        }
    }

    /// TODO: Here we want to save the data in `self.cached`, but we also wan to
    /// reuse the old cached data, so we can do less work if the file is in fact
    /// identical.
    fn load(&self, repo: &GitRepo) -> Result<(), LoaderError> {
        let items = match &self.loader {
            ContentStrategy::Glob(glob) => glob.read(repo)?,
        };

        let mut cached = self.cached.lock().unwrap();
        for item in items {
            cached.insert(item.file.to_string(), item);
        }

        Ok(())
    }

    fn load_once(&self, repo: &GitRepo, path: &Utf8Path) -> Result<(), LoaderError> {
        let item = match &self.loader {
            ContentStrategy::Glob(loader) => loader.read_once(path, repo)?,
        };

        let mut cached = self.cached.lock().unwrap();
        if let Some(item) = item {
            cached.insert(item.file.to_string(), item);
        }

        Ok(())
    }
}

// ASSETS LOADER
// =============

type BoxAny = Box<dyn Any + Send + Sync>;
type BoxFn8 = Box<dyn Fn(&[u8]) -> ArcAny + Send + Sync>;

struct AssetsGlobFn(BoxFn8);

impl Debug for AssetsGlobFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fn(*)")
    }
}

#[derive(Debug)]
pub struct Assets(AssetsLoader);

#[derive(Debug)]
enum AssetsLoader {
    Glob {
        path_base: &'static str,
        path_glob: &'static str,
        func: AssetsGlobFn,
    },
    GlobDefer {
        path_base: &'static str,
        path_glob: &'static str,
        func: fn(&[u8]) -> Vec<u8>,
    },
}

impl Assets {
    pub fn glob<T>(path_base: &'static str, path_glob: &'static str, call: fn(&[u8]) -> T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self(AssetsLoader::Glob {
            path_base,
            path_glob,
            func: AssetsGlobFn(Box::new(move |data| Arc::new(call(data)))),
        })
    }

    pub fn glob_defer(
        path_base: &'static str,
        path_glob: &'static str,
        func: fn(&[u8]) -> Vec<u8>,
    ) -> Self {
        Self(AssetsLoader::GlobDefer {
            path_base,
            path_glob,
            func,
        })
    }

    /// Load all assets which are matched by the defined glob.
    fn load(&self) -> Vec<InputItem> {
        match &self.0 {
            AssetsLoader::Glob {
                path_base,
                path_glob,
                func: AssetsGlobFn(func),
            } => {
                let pattern = Utf8Path::new(path_base).join(path_glob);
                let iter = glob::glob(pattern.as_str()).unwrap();

                let mut out = vec![];
                for entry in iter {
                    let entry = Utf8PathBuf::try_from(entry.unwrap()).unwrap();
                    let bytes = fs::read(&entry).unwrap();

                    let hash = Hash32::hash(&bytes);
                    let data = func(&bytes);

                    out.push(InputItem {
                        hash,
                        file: entry.to_owned(),
                        slug: entry.strip_prefix(path_base).unwrap_or(&entry).to_owned(),
                        data: Input::InMemory(data),
                        info: None,
                    });
                }

                out
            }
            AssetsLoader::GlobDefer {
                path_base,
                path_glob,
                func,
            } => {
                let pattern = Utf8Path::new(path_base).join(path_glob);
                let iter = glob::glob(pattern.as_str()).unwrap();

                let mut out = vec![];
                for entry in iter {
                    let entry = Utf8PathBuf::try_from(entry.unwrap()).unwrap();
                    let bytes = fs::read(&entry).unwrap();

                    let hash = Hash32::hash(&bytes);

                    out.push(InputItem {
                        hash,
                        file: entry.to_owned(),
                        slug: entry.strip_prefix(path_base).unwrap_or(&entry).to_owned(),
                        data: Input::OnDisk(*func),
                        info: None,
                    });
                }

                out
            }
        }
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

#[derive(Debug, Clone)]
struct InputContent {
    area: Utf8PathBuf,
    meta: Arc<dyn Any + Send + Sync>,
    text: String,
}

#[derive(Debug, Clone)]
struct InputStylesheet {
    stylesheet: String,
}

#[derive(Debug, Clone)]
enum Input {
    Content(InputContent),
    InMemory(Arc<dyn Any + Send + Sync>),
    OnDisk(fn(&[u8]) -> Vec<u8>),
    Stylesheet(InputStylesheet),
    Script,
}

#[derive(Debug, Clone)]
struct InputItem {
    hash: Hash32,
    file: Utf8PathBuf,
    slug: Utf8PathBuf,
    data: Input,
    info: Option<gitmap::GitInfo>,
}

#[derive(Debug)]
struct Trace<D>
where
    D: Send + Sync,
{
    task: Task<D>,
    init: bool,
    deps: HashMap<Utf8PathBuf, Hash32>,
    glob: Vec<glob::Pattern>,
    path: Vec<(Utf8PathBuf, String)>,
}

impl<G: Send + Sync> Trace<G> {
    fn new(task: Task<G>) -> Self {
        Self {
            task,
            init: true,
            deps: HashMap::new(),
            glob: Vec::new(),
            path: Vec::new(),
        }
    }

    fn new_with(&self, deps: Tracker, path: Vec<(Utf8PathBuf, String)>) -> Self {
        Self {
            task: self.task.clone(),
            init: false,
            deps: deps.hash,
            glob: deps.glob,
            path,
        }
    }

    fn is_outdated(&self, inputs: &HashMap<Utf8PathBuf, InputItem>) -> bool {
        if self.init {
            return true;
        }

        let mut cache_hits = 0;
        for item in inputs.values() {
            if let Some(hash_old) = self.deps.get(&item.file) {
                if item.hash == *hash_old {
                    cache_hits += 1;
                    continue;
                } else {
                    return true;
                }
            }

            // If we haven't had a file dependency, but it matches, it means it
            // was recently added by the user.
            for pattern in &self.glob {
                if pattern.matches_path(item.slug.as_ref()) {
                    return true;
                }
            }
        }

        // If any file dependency is physically removed, the cache hit count
        // will not match the old dependency count
        cache_hits != self.deps.len()
    }
}

#[derive(Debug)]
struct Scheduler<'a, D>
where
    D: Send + Sync,
{
    context: &'a Globals<D>,
    builder: Arc<RwLock<Builder>>,
    tracked: Vec<Trace<D>>,
    items: HashMap<Utf8PathBuf, InputItem>,
    cache_pages: HashMap<Utf8PathBuf, Hash32>,
}

impl<'a, D: Send + Sync> Scheduler<'a, D> {
    pub fn new(website: &'a Website<D>, context: &'a Globals<D>, items: Vec<InputItem>) -> Self {
        Self {
            context,
            builder: Arc::new(RwLock::new(Builder::new())),
            tracked: website.tasks.iter().cloned().map(Trace::new).collect(),
            items: HashMap::from_iter(items.into_iter().map(|item| (item.file.clone(), item))),
            cache_pages: HashMap::new(),
        }
    }

    pub fn update(&mut self, inputs: Vec<InputItem>) {
        for input in inputs {
            self.items.insert(input.file.clone(), input);
        }
    }

    pub fn build_sitemap(&self, opts: &Utf8Path) -> Vec<u8> {
        let urls = self
            .tracked
            .iter()
            .flat_map(|x| &x.path)
            .collect::<HashSet<_>>()
            .into_iter()
            .map(|path| {
                Url::builder(opts.join(&path.0).parent().unwrap().to_string())
                    .change_frequency(ChangeFrequency::Monthly)
                    .priority(0.8)
                    .build()
                    .expect("failed a <url> validation")
            })
            .collect::<Vec<_>>();
        let urls = UrlSet::new(urls).expect("failed a <urlset> validation");
        let mut buf = Vec::<u8>::new();
        urls.write(&mut buf).expect("failed to write XML");
        buf
    }

    fn rebuild_trace(&self, trace: Trace<D>) -> Result<Trace<D>, BuilderError> {
        if !trace.is_outdated(&self.items) {
            return Ok(trace);
        }

        let tracker = Tracker {
            hash: HashMap::new(),
            glob: Vec::new(),
        };

        let tracker = Rc::new(RefCell::new(tracker));

        let paths = trace.task.call(Context {
            globals: self.context,
            builder: self.builder.clone(),
            tracker: tracker.clone(),
            items: &self.items,
        })?;

        let tracker = Rc::unwrap_or_clone(tracker).into_inner();

        Ok(trace.new_with(tracker, paths))
    }

    fn remove(&mut self, paths: HashSet<&Path>) {
        self.items = mem::take(&mut self.items)
            .into_iter()
            .filter(|p| !paths.contains(p.1.file.as_std_path()))
            .collect();
    }

    fn refresh(&mut self) -> Result<(), HauchiwaError> {
        self.build_pages()?;
        self.write_pages()?;

        Ok(())
    }

    fn build_pages(&mut self) -> Result<(), BuilderError> {
        let traces = mem::take(&mut self.tracked);

        let pb = ProgressBar::new(traces.len() as u64);
        pb.set_message("Running build tasks...");
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .expect("Error setting progress bar template")
                .progress_chars("#>-"),
        );

        let s = Instant::now();
        self.tracked = traces
            .into_par_iter()
            .progress_with(pb.clone())
            .map(|trace| self.rebuild_trace(trace))
            .collect::<Result<_, _>>()?;

        pb.finish_with_message(format!(
            "Finished running build tasks! {}",
            crate::io::as_overhead(s)
        ));

        Ok(())
    }

    fn write_pages(&mut self) -> Result<(), HauchiwaError> {
        let mut temp = HashMap::new();

        let paths: Vec<_> = self
            .tracked
            .iter()
            .flat_map(|trace| &trace.path)
            .filter_map(|(path, data)| {
                let hash = Hash32::hash(data);
                let path = Utf8Path::new("dist").join(path);

                if Some(hash) == self.cache_pages.get(&path).copied() {
                    None
                } else {
                    self.cache_pages.insert(path.clone(), hash);
                    Some((path, data, hash))
                }
            })
            .collect();

        if paths.is_empty() {
            println!(
                "{}",
                style("No generated pages to write. Skipping.").green()
            );
            return Ok(());
        }

        let pb = ProgressBar::new(paths.len() as u64);
        pb.set_message("Writing generated pages...");
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .expect("Error setting progress bar template")
                .progress_chars("#>-"),
        );

        let s = Instant::now();
        paths
            .into_iter()
            .progress_with(pb.clone())
            .try_for_each::<_, Result<_, BuilderError>>(|(path, data, hash)| {
                if temp.contains_key(&path) {
                    println!("Warning, overwriting path {path}")
                }

                if let Some(dir) = path.parent() {
                    fs::create_dir_all(dir)
                        .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
                }
                let mut file = fs::File::create(&path)
                    .map_err(|e| BuilderError::FileWriteError(path.to_owned(), e))?;
                std::io::Write::write_all(&mut file, data.as_bytes())
                    .map_err(|e| BuilderError::FileWriteError(path.to_owned(), e))?;

                temp.insert(path.clone(), hash);

                Ok(())
            })?;

        pb.finish_with_message(format!(
            "Finished writing generated pages! {}",
            crate::io::as_overhead(s)
        ));

        self.cache_pages.extend(temp);

        Ok(())
    }

    fn fulfill_build_requests(&self) -> Result<(), BuilderError> {
        let queue = mem::take(&mut self.builder.write().unwrap().queue);

        let pb = ProgressBar::new(queue.len() as u64);
        pb.set_message("Building requested assets...");
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .expect("Error setting progress bar template")
                .progress_chars("#>-"),
        );

        let s = Instant::now();
        queue
            .into_par_iter()
            .progress_with(pb.clone())
            .try_for_each::<_, Result<_, BuilderError>>(|item| {
                let BuildRequest {
                    path_file,
                    path_temp,
                    path_dist,
                    call,
                } = item;

                if !path_temp.exists() {
                    let buffer = fs::read(&path_file)
                        .map_err(|e| BuilderError::FileReadError(path_file, e))?;
                    let buffer = call(&buffer);
                    fs::create_dir_all(".cache/hash")
                        .map_err(|e| BuilderError::CreateDirError(".cache/hash".into(), e))?;
                    fs::write(&path_temp, buffer)
                        .map_err(|e| BuilderError::FileWriteError(path_temp.clone(), e))?;
                }

                let dir = path_dist.parent().unwrap_or(&path_dist);
                fs::create_dir_all(dir) //
                    .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
                fs::copy(&path_temp, &path_dist).map_err(|e| {
                    BuilderError::FileCopyError(path_temp.clone(), path_dist.clone(), e)
                })?;

                Ok(())
            })?;

        pb.finish_with_message(format!(
            "Finished building requested assets! {}",
            crate::io::as_overhead(s)
        ));

        Ok(())
    }
}

// ******************************
// *          Builder           *
// ******************************

#[derive(Debug)]
struct BuildRequest {
    path_file: Utf8PathBuf,
    path_temp: Utf8PathBuf,
    path_dist: Utf8PathBuf,
    call: fn(&[u8]) -> Vec<u8>,
}

#[derive(Debug)]
struct Builder {
    /// Paths to files in dist
    dist: HashMap<Hash32, Utf8PathBuf>,
    /// Build queue
    queue: Vec<BuildRequest>,
}

impl Builder {
    fn new() -> Self {
        Self {
            dist: HashMap::new(),
            queue: Vec::new(),
        }
    }

    fn check(&self, hash: Hash32) -> Option<Utf8PathBuf> {
        self.dist.get(&hash).cloned()
    }

    fn request_deferred(
        &mut self,
        hash: Hash32,
        path: &Utf8Path,
        call: fn(&[u8]) -> Vec<u8>,
    ) -> Result<Utf8PathBuf, BuilderError> {
        let hash_hex = hash.to_hex();

        let path_temp = Utf8Path::new(".cache/hash").join(&hash_hex);
        let path_dist = Utf8Path::new("dist/hash").join(&hash_hex);
        let path_root = Utf8Path::new("/hash/").join(&hash_hex);

        let request = BuildRequest {
            path_file: path.to_owned(),
            path_temp,
            path_dist,
            call,
        };

        self.queue.push(request);
        self.dist.insert(hash, path_root.clone());
        Ok(path_root)
    }

    fn request_stylesheet(
        &mut self,
        hash: Hash32,
        style: &InputStylesheet,
    ) -> Result<Utf8PathBuf, BuilderError> {
        let hash_hex = hash.to_hex();
        let path = Utf8Path::new("hash").join(&hash_hex).with_extension("css");

        let path_root = Utf8Path::new("/").join(&path);
        let path_dist = Utf8Path::new("dist").join(&path);

        let dir = path_dist.parent().unwrap_or(&path_dist);
        fs::create_dir_all(dir) //
            .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
        fs::write(&path_dist, &style.stylesheet)
            .map_err(|e| BuilderError::FileWriteError(path_dist.clone(), e))?;

        self.dist.insert(hash, path_root.clone());
        Ok(path_root)
    }

    fn request_script(
        &mut self,
        hash: Hash32,
        file: &Utf8Path,
    ) -> Result<Utf8PathBuf, BuilderError> {
        let hash_hex = hash.to_hex();
        let path = Utf8Path::new("hash").join(&hash_hex).with_extension("js");

        let path_root = Utf8Path::new("/").join(&path);
        let path_dist = Utf8Path::new("dist").join(&path);

        let dir = path_dist.parent().unwrap_or(&path_dist);
        fs::create_dir_all(dir) //
            .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
        fs::copy(file, &path_dist)
            .map_err(|e| BuilderError::FileCopyError(file.to_owned(), path_dist.clone(), e))?;

        self.dist.insert(hash, path_root.clone());
        Ok(path_root)
    }
}

// ******************************
// *          Runtime           *
// ******************************

/// A simple wrapper for all context data passed at runtime to tasks defined for
/// the website. Use this struct's methods to query required data.
pub struct Context<'a, G>
where
    G: Send + Sync,
{
    /// Global data for the current build.
    globals: &'a Globals<G>,
    /// Builder allows scheduling build requests.
    builder: Arc<RwLock<Builder>>,
    /// Tracked dependencies for current instantation.
    tracker: Rc<RefCell<Tracker>>,
    /// Every single input.
    items: &'a HashMap<Utf8PathBuf, InputItem>,
}

#[derive(Debug)]
pub struct QueryContent<'a, D> {
    pub file: &'a Utf8Path,
    pub slug: &'a Utf8Path,
    pub area: &'a Utf8Path,
    pub meta: &'a D,
    pub info: Option<&'a GitInfo>,
    pub content: &'a str,
}

#[derive(Clone)]
struct Tracker {
    hash: HashMap<Utf8PathBuf, Hash32>,
    glob: Vec<glob::Pattern>,
}

impl<'a, G> Context<'a, G>
where
    G: Send + Sync,
{
    /// Retrieve the globals.
    pub fn get_globals(&self) -> &Globals<G> {
        self.globals
    }

    /// Retrieve a single page by glob pattern and metadata shape.
    pub fn get_page<D>(&self, pattern: &str) -> Result<QueryContent<'_, D>, HauchiwaError>
    where
        D: 'static,
    {
        let glob = glob::Pattern::new(pattern)?;
        self.tracker.borrow_mut().glob.push(glob.clone());

        let item = self
            .items
            .values()
            .find(|item| glob.matches_path(item.slug.as_ref()))
            .ok_or_else(|| HauchiwaError::AssetNotFound(glob.to_string()))?;

        if let Input::Content(content) = &item.data {
            let meta = content
                .meta
                .downcast_ref::<D>()
                .ok_or_else(|| HauchiwaError::Frontmatter(item.file.to_string()))?;
            let area = content.area.as_ref();
            let content = content.text.as_str();

            self.tracker
                .borrow_mut()
                .hash
                .insert(item.file.clone(), item.hash);

            Ok(QueryContent {
                file: &item.file,
                slug: &item.slug,
                area,
                meta,
                info: item.info.as_ref(),
                content,
            })
        } else {
            Err(HauchiwaError::AssetNotFound(glob.to_string()))
        }
    }

    /// Retrieve many possible content items.
    pub fn get_pages<D>(&self, pattern: &str) -> Result<Vec<QueryContent<'_, D>>, HauchiwaError>
    where
        D: 'static,
    {
        let pattern = glob::Pattern::new(pattern)?;
        self.tracker.borrow_mut().glob.push(pattern.clone());

        let inputs: Vec<_> = self
            .items
            .values()
            .filter(|item| pattern.matches_path(item.slug.as_ref()))
            .collect();

        let mut tracked = self.tracker.borrow_mut();
        for input in inputs.iter() {
            tracked.hash.insert(input.file.clone(), input.hash);
        }

        let query = inputs
            .into_iter()
            .filter_map(|item| {
                let (area, meta, content) = match &item.data {
                    Input::Content(input_content) => {
                        let area = input_content.area.as_ref();
                        let meta = input_content.meta.downcast_ref::<D>()?;
                        let data = input_content.text.as_str();
                        Some((area, meta, data))
                    }
                    _ => None,
                }?;

                Some(QueryContent {
                    file: &item.file,
                    slug: &item.slug,
                    area,
                    meta,
                    info: item.info.as_ref(),
                    content,
                })
            })
            .collect();

        Ok(query)
    }

    /// Get compiled CSS style by alias.
    pub fn get_styles(&self, path: &Utf8Path) -> Result<Utf8PathBuf, HauchiwaError> {
        let item = self
            .items
            .values()
            .find(|item| item.file == path)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))?;

        if let Input::Stylesheet(style) = &item.data {
            let path = self
                .builder
                .read()
                .map_err(|_| HauchiwaError::LockRead)?
                .check(item.hash);

            self.tracker
                .borrow_mut()
                .hash
                .insert(item.file.clone(), item.hash);

            let path = match path {
                Some(path) => path,
                None => self
                    .builder
                    .write()
                    .map_err(|_| HauchiwaError::LockWrite)?
                    .request_stylesheet(item.hash, style)?,
            };

            Ok(path)
        } else {
            Err(HauchiwaError::AssetNotFound(path.to_string()))
        }
    }

    /// Get path to a generated asset file.
    pub fn get_asset_deferred(&self, path: &Utf8Path) -> Result<Utf8PathBuf, HauchiwaError> {
        let input = self
            .items
            .values()
            .find(|item| item.file == path)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))?;

        if let Input::OnDisk(call) = &input.data {
            let res = self
                .builder
                .read()
                .map_err(|_| HauchiwaError::LockRead)?
                .check(input.hash);
            if let Some(res) = res {
                return Ok(res);
            }

            let res = self
                .builder
                .write()
                .map_err(|_| HauchiwaError::LockWrite)?
                .request_deferred(input.hash, &input.file, *call)?;

            self.tracker
                .borrow_mut()
                .hash
                .insert(input.file.clone(), input.hash);

            Ok(res)
        } else {
            Err(HauchiwaError::AssetNotFound(path.to_string()))
        }
    }

    pub fn get_script(&self, path: &str) -> Result<Utf8PathBuf, HauchiwaError> {
        let path = Utf8Path::new(".cache/scripts/")
            .join(path)
            .with_extension("js");

        let input = self
            .items
            .values()
            .find(|item| item.file == path)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))?;

        if let Input::Script = &input.data {
            let res = self
                .builder
                .read()
                .map_err(|_| HauchiwaError::LockRead)?
                .check(input.hash);

            if let Some(res) = res {
                return Ok(res);
            }

            let res = self
                .builder
                .write()
                .map_err(|_| HauchiwaError::LockWrite)?
                .request_script(input.hash, &input.file)?;

            self.tracker
                .borrow_mut()
                .hash
                .insert(input.file.clone(), input.hash);

            Ok(res)
        } else {
            Err(HauchiwaError::AssetNotFound(path.to_string()))
        }
    }

    pub fn get_asset_any<T>(&self, area: &Utf8Path) -> Result<Option<&T>, HauchiwaError>
    where
        T: 'static,
    {
        let glob = format!("{}/*", area);
        let glob = glob::Pattern::new(&glob)?;
        let opts = glob::MatchOptions {
            case_sensitive: true,
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };

        let found = self
            .items
            .values()
            .filter(|item| glob.matches_path_with(item.file.as_std_path(), opts))
            .find_map(|item| match &item.data {
                Input::InMemory(any) => {
                    let data = any.downcast_ref::<T>()?;
                    let file = item.file.clone();
                    let hash = item.hash;
                    Some((data, file, hash))
                }
                _ => None,
            });

        if let Some((data, file, hash)) = found {
            self.tracker.borrow_mut().hash.insert(file, hash);
            return Ok(Some(data));
        }

        Ok(None)
    }
}
