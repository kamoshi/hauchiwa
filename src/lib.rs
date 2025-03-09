#![doc = include_str!("../README.md")]
mod builder;
mod collection;
mod error;
mod generator;
mod watch;

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use builder::{Input, InputItem, InputStylesheet, Scheduler};
use camino::{Utf8Path, Utf8PathBuf};
use error::{CleanError, HookError, SitemapError, StylesheetError};
use generator::load_scripts;
use gray_matter::engine::{JSON, YAML};
use gray_matter::Matter;
use sha2::{Digest, Sha256};

pub use crate::collection::Collection;
pub use crate::error::{BuilderError, HauchiwaError};
pub use crate::generator::Sack;

/// This value controls whether the library should run in the *build* or the
/// *watch* mode. In *build* mode, the library builds every page of the website
/// just once and stops. In *watch* mode, the library initializes the initial
/// state of the build process, opens up a websocket port, and watches for any
/// changes in the file system. Using the *watch* mode allows you to enable
/// live-reload while editing the styles or the content of your website.
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    /// Run the library in *build* mode.
    Build,
    /// Run the library in *watch* mode.
    Watch,
}

/// `D` represents any additional data that should be globally available
/// during the rendering process.
#[derive(Debug, Clone)]
pub struct Context<D: Send + Sync> {
    pub mode: Mode,
    pub data: D,
}

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

/// This struct represents the website which will be built by the generator. The individual
/// settings can be set by calling the `setup` function.
///
/// The `G` type parameter is the global data container accessible in every page renderer as `ctx.data`,
/// though it can be replaced with the `()` Unit if you don't need to pass any data.
#[derive(Debug)]
pub struct Website<D: Send + Sync> {
    /// Rendered assets and content are outputted to this directory.
    /// All collections added to this website.
    pub(crate) collections: Vec<Collection>,
    /// Preprocessors for files
    pub(crate) processors: Vec<Processor>,
    /// Build tasks which can be used to generate pages.
    pub(crate) tasks: Vec<Task<D>>,
    /// Global scripts
    pub(crate) global_scripts: HashMap<&'static str, &'static str>,
    /// Global styles
    pub(crate) global_styles: Vec<Utf8PathBuf>,
    /// Sitemap options
    pub(crate) opts_sitemap: Option<Utf8PathBuf>,
    /// Hooks
    pub(crate) hooks: Vec<Hook>,
}

impl<D: Send + Sync + 'static> Website<D> {
    pub fn configure() -> WebsiteConfiguration<D> {
        WebsiteConfiguration::new()
    }

    pub fn build(&self, data: D) -> Result<(), HauchiwaError> {
        let context = Context {
            mode: Mode::Build,
            data,
        };

        let _ = init(self, &context)?;

        Ok(())
    }

    pub fn watch(&self, data: D) -> Result<(), HauchiwaError> {
        let context = Context {
            mode: Mode::Watch,
            data,
        };

        let scheduler = init(self, &context)?;
        crate::watch::watch(self, scheduler)?;

        Ok(())
    }
}

fn init<'a, D>(
    website: &'a Website<D>,
    context: &'a Context<D>,
) -> Result<Scheduler<'a, D>, HauchiwaError>
where
    D: Send + Sync + 'static,
{
    init_clean_dist()?;
    init_clone_static()?;

    let styles = css_load_paths(&website.global_styles)?;
    let script = load_scripts(&website.global_scripts);

    let items: Vec<_> = website
        .collections
        .iter()
        .flat_map(|collection| collection.load(&website.processors))
        .chain(styles)
        .chain(script)
        .collect();

    let mut scheduler = Scheduler::new(website, context, items);
    scheduler.build()?;

    build_hooks(website, &scheduler)?;
    build_sitemap(website, &scheduler)?;

    Ok(scheduler)
}

fn init_clean_dist() -> Result<(), CleanError> {
    println!("Cleaning dist");
    if fs::metadata("dist").is_ok() {
        fs::remove_dir_all("dist").map_err(|e| CleanError::RemoveError(e))?;
    }
    fs::create_dir("dist").map_err(|e| CleanError::CreateError(e))?;
    Ok(())
}

fn init_clone_static() -> Result<(), HauchiwaError> {
    copy_rec(Utf8Path::new("public"), Utf8Path::new("dist"))
        .map_err(|e| HauchiwaError::CloneStatic(e))
}

fn copy_rec(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let filetype = entry.file_type()?;
        if filetype.is_dir() {
            copy_rec(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn build_hooks<D>(website: &Website<D>, scheduler: &Scheduler<D>) -> Result<(), HookError>
where
    D: Send + Sync,
{
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

    Ok(())
}

fn build_sitemap<D>(website: &Website<D>, scheduler: &Scheduler<D>) -> Result<(), SitemapError>
where
    D: Send + Sync + 'static,
{
    if let Some(ref opts) = website.opts_sitemap {
        let sitemap = scheduler.build_sitemap(opts);
        fs::write("dist/sitemap.xml", sitemap)?;
    }
    Ok(())
}

fn css_load_paths(paths: &[Utf8PathBuf]) -> Result<Vec<InputItem>, StylesheetError> {
    let mut items = Vec::new();

    for path in paths {
        let pattern = path.join("**/[!_]*.scss");
        let results = glob::glob(pattern.as_str())?;

        for path in results {
            let item = css_compile(path?)?;
            items.push(item);
        }
    }

    Ok(items)
}

fn css_compile(file: PathBuf) -> Result<InputItem, StylesheetError> {
    let opts = grass::Options::default().style(grass::OutputStyle::Compressed);

    let file =
        Utf8PathBuf::try_from(file).map_err(|e| StylesheetError::InvalidFileName(e.to_string()))?;
    let stylesheet =
        grass::from_path(&file, &opts).map_err(|e| StylesheetError::Compiler(e.to_string()))?;

    Ok(InputItem {
        hash: Sha256::digest(&stylesheet).into(),
        file: file.clone(),
        slug: file,
        data: Input::Stylesheet(InputStylesheet { stylesheet }),
    })
}

/// A builder struct for creating a `Website` with specified settings.
#[derive(Debug, Default)]
pub struct WebsiteConfiguration<G: Send + Sync> {
    collections: Vec<Collection>,
    processors: Vec<Processor>,
    tasks: Vec<Task<G>>,
    global_scripts: HashMap<&'static str, &'static str>,
    global_styles: Vec<Utf8PathBuf>,
    opts_sitemap: Option<Utf8PathBuf>,
    hooks: Vec<Hook>,
}

impl<D: Send + Sync + 'static> WebsiteConfiguration<D> {
    fn new() -> Self {
        Self {
            collections: Vec::default(),
            processors: Vec::default(),
            tasks: Vec::default(),
            global_scripts: HashMap::default(),
            global_styles: Vec::default(),
            opts_sitemap: None,
            hooks: Vec::new(),
        }
    }

    pub fn add_collections(mut self, collections: impl IntoIterator<Item = Collection>) -> Self {
        self.collections.extend(collections);
        self
    }

    pub fn add_processors(mut self, processors: impl IntoIterator<Item = Processor>) -> Self {
        self.processors.extend(processors);
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

    pub fn add_task(mut self, fun: fn(Sack<D>) -> TaskResult<TaskPaths>) -> Self {
        self.tasks.push(Task::new(fun));
        self
    }

    pub fn set_opts_sitemap(mut self, path: impl AsRef<str>) -> Self {
        self.opts_sitemap = Some(path.as_ref().into());
        self
    }

    pub fn finish(self) -> Website<D> {
        Website {
            collections: self.collections,
            processors: self.processors,
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

#[derive(Debug)]
pub struct QueryContent<'a, D> {
    pub file: &'a Utf8Path,
    pub slug: &'a Utf8Path,
    pub area: &'a Utf8Path,
    pub meta: &'a D,
    pub content: &'a str,
}

type Erased = Box<dyn Any + Send + Sync>;

pub(crate) enum ProcessorKind {
    Asset(Box<dyn Fn(&str) -> Erased>),
    Image,
}

impl Debug for ProcessorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessorKind::Asset(_) => write!(f, "<Processor Asset>"),
            ProcessorKind::Image => write!(f, "<Processor Image>"),
        }
    }
}

#[derive(Debug)]
pub struct Processor {
    exts: HashSet<&'static str>,
    kind: ProcessorKind,
}

impl Processor {
    pub fn process_assets<T: Send + Sync + 'static>(
        exts: impl IntoIterator<Item = &'static str>,
        call: fn(&str) -> T,
    ) -> Self {
        Self {
            exts: HashSet::from_iter(exts),
            kind: ProcessorKind::Asset(Box::new(move |data| Box::new(call(data)))),
        }
    }

    pub fn process_images(exts: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            exts: HashSet::from_iter(exts),
            kind: ProcessorKind::Image,
        }
    }
}

/// Rendered content from the userland.
type TaskPaths = Vec<(Utf8PathBuf, String)>;

/// Result from a single executed task.
pub type TaskResult<T> = anyhow::Result<T, anyhow::Error>;

/// Task function pointer used to dynamically generate a website page. This
/// function is provided by the user from the userland, but it is used
/// internally during the build process.
type TaskFnPtr<D> = Arc<dyn Fn(Sack<D>) -> TaskResult<TaskPaths> + Send + Sync>;

/// Wraps `TaskFnPtr` and implements `Debug` trait for function pointer.
struct Task<D: Send + Sync>(TaskFnPtr<D>);

impl<D: Send + Sync> Task<D> {
    /// Create new task function pointer.
    fn new<F>(func: F) -> Self
    where
        D: Send + Sync,
        F: Fn(Sack<D>) -> TaskResult<TaskPaths> + Send + Sync + 'static,
    {
        Self(Arc::new(func))
    }

    /// Run the task to generate a page.
    fn run(&self, sack: Sack<D>) -> TaskResult<TaskPaths> {
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

/// Generate the functions used to initialize content files. These functions can
/// be used to parse the front matter using engines from crate `gray_matter`.
macro_rules! matter_parser {
	($name:ident, $engine:path) => {
		#[doc = concat!(
			"This function can be used to extract metadata from a document with `D` as the frontmatter shape.\n",
			"Configured to use [`", stringify!($engine), "`] as the engine of the parser."
		)]
		pub fn $name<D>(content: &str) -> (D, String)
		where
			D: for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
		{
			// We can cache the creation of the parser
			static PARSER: LazyLock<Matter<$engine>> = LazyLock::new(Matter::<$engine>::new);

			let result = PARSER.parse_with_struct::<D>(content).unwrap();
			(
				// Just the front matter
				result.data,
				// The rest of the content
				result.content,
			)
		}
	};
}

matter_parser!(parse_matter_yaml, YAML);
matter_parser!(parse_matter_json, JSON);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Hash32([u8; 32]);

impl Hash32 {
    fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut acc = vec![0u8; 64];

        for (i, &byte) in self.0.iter().enumerate() {
            acc[i * 2 + 0] = HEX[(byte >> 04) as usize];
            acc[i * 2 + 1] = HEX[(byte & 0xF) as usize];
        }

        // SAFETY: `acc` contains only valid ASCII bytes.
        unsafe { String::from_utf8_unchecked(acc) }
    }
}

impl<T> From<T> for Hash32
where
    T: Into<[u8; 32]>,
{
    fn from(value: T) -> Self {
        Hash32(value.into())
    }
}

fn optimize_image(buffer: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(buffer).expect("Couldn't load image");
    let w = img.width();
    let h = img.height();

    let mut out = Vec::new();
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);

    encoder
        .encode(&img.to_rgba8(), w, h, image::ExtendedColorType::Rgba8)
        .expect("Encoding error");

    out
}

#[derive(Debug)]
pub(crate) struct Builder {
    /// Paths to files in dist
    dist: HashMap<Hash32, Utf8PathBuf>,
}

impl Builder {
    pub(crate) fn new() -> Self {
        Self {
            dist: HashMap::new(),
        }
    }

    pub(crate) fn check(&self, hash: Hash32) -> Option<Utf8PathBuf> {
        self.dist.get(&hash).cloned()
    }

    pub(crate) fn build_image(
        &mut self,
        hash: Hash32,
        file: &Utf8Path,
    ) -> Result<Utf8PathBuf, BuilderError> {
        let hash_hex = hash.to_hex();
        let path = Utf8Path::new("hash").join(&hash_hex).with_extension("webp");
        let path_cache = Utf8Path::new(".cache").join(&path);

        if !path_cache.exists() {
            let buffer =
                fs::read(file).map_err(|e| BuilderError::FileReadError(file.to_owned(), e))?;
            let buffer = optimize_image(&buffer);
            fs::create_dir_all(".cache/hash")
                .map_err(|e| BuilderError::CreateDirError(".cache/hash".into(), e))?;
            fs::write(&path_cache, buffer)
                .map_err(|e| BuilderError::FileWriteError(path_cache.clone(), e))?;
        }

        let path_root = Utf8Path::new("/").join(&path);
        let path_dist = Utf8Path::new("dist").join(&path);

        println!("IMG: {}", path_dist);
        let dir = path_dist.parent().unwrap_or(&path_dist);
        fs::create_dir_all(dir) //
            .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
        fs::copy(&path_cache, &path_dist)
            .map_err(|e| BuilderError::FileCopyError(path_cache.clone(), path_dist.clone(), e))?;

        self.dist.insert(hash, path_root.clone());
        Ok(path_root)
    }

    pub(crate) fn build_style(
        &mut self,
        hash: Hash32,
        style: &InputStylesheet,
    ) -> Result<Utf8PathBuf, BuilderError> {
        let hash_hex = hash.to_hex();
        let path = Utf8Path::new("hash").join(&hash_hex).with_extension("css");

        let path_root = Utf8Path::new("/").join(&path);
        let path_dist = Utf8Path::new("dist").join(&path);

        println!("CSS: {}", path_dist);
        let dir = path_dist.parent().unwrap_or(&path_dist);
        fs::create_dir_all(dir) //
            .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
        fs::write(&path_dist, &style.stylesheet)
            .map_err(|e| BuilderError::FileWriteError(path_dist.clone(), e))?;

        self.dist.insert(hash, path_root.clone());
        Ok(path_root)
    }

    pub(crate) fn build_script(
        &mut self,
        hash: Hash32,
        file: &Utf8Path,
    ) -> Result<Utf8PathBuf, BuilderError> {
        let hash_hex = hash.to_hex();
        let path = Utf8Path::new("hash").join(&hash_hex).with_extension("js");

        let path_root = Utf8Path::new("/").join(&path);
        let path_dist = Utf8Path::new("dist").join(&path);

        println!("JS: {}", path_dist);
        let dir = path_dist.parent().unwrap_or(&path_dist);
        fs::create_dir_all(dir) //
            .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
        fs::copy(file, &path_dist)
            .map_err(|e| BuilderError::FileCopyError(file.to_owned(), path_dist.clone(), e))?;

        self.dist.insert(hash, path_root.clone());
        Ok(path_root)
    }
}
