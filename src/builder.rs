use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::io::Write;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::{fs, mem};

use camino::{Utf8Path, Utf8PathBuf};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use sitemap_rs::url::{ChangeFrequency, Url};
use sitemap_rs::url_set::UrlSet;

use crate::generator::Sack;
use crate::{Builder, BuilderError, Context, Hash32, Task, Website};

/// Init pointer used to dynamically retrieve front matter. The type of front matter
/// needs to be erased at run time and this is one way of accomplishing this,
/// it's hidden behind the `dyn Fn` existential type.
type InitFnPtr = Arc<dyn Fn(&str) -> (Arc<dyn Any + Send + Sync>, String)>;

/// Wraps `InitFnPtr` and implements `Debug` trait for function pointer.
#[derive(Clone)]
pub(crate) struct InitFn(pub(crate) InitFnPtr);

impl InitFn {
    /// Call the contained `InitFn` pointer.
    pub(crate) fn call(&self, data: &str) -> (Arc<dyn Any + Send + Sync>, String) {
        (self.0)(data)
    }
}

impl Debug for InitFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InitFn(*)")
    }
}

#[derive(Debug)]
pub(crate) struct InputContent {
    pub(crate) area: Utf8PathBuf,
    pub(crate) meta: Arc<dyn Any + Send + Sync>,
    pub(crate) text: String,
}

#[derive(Debug)]
pub(crate) struct InputStylesheet {
    pub(crate) stylesheet: String,
}

#[derive(Debug)]
pub(crate) enum Input {
    Content(InputContent),
    Asset(Box<dyn Any + Send + Sync>),
    Picture,
    Stylesheet(InputStylesheet),
    Script,
}

#[derive(Debug)]
pub(crate) struct InputItem {
    pub(crate) hash: Hash32,
    pub(crate) file: Utf8PathBuf,
    pub(crate) slug: Utf8PathBuf,
    pub(crate) data: Input,
}

#[derive(Debug)]
struct Trace<D>
where
    D: Send + Sync,
{
    task: Task<D>,
    init: bool,
    deps: HashMap<Utf8PathBuf, Hash32>,
    path: Box<[Utf8PathBuf]>,
}

impl<G: Send + Sync> Trace<G> {
    fn new(task: Task<G>) -> Self {
        Self {
            task,
            init: true,
            deps: HashMap::new(),
            path: Box::new([]),
        }
    }

    fn new_with(&self, deps: HashMap<Utf8PathBuf, Hash32>, path: Box<[Utf8PathBuf]>) -> Self {
        Self {
            task: self.task.clone(),
            init: false,
            deps,
            path,
        }
    }

    fn is_outdated(&self, inputs: &HashMap<Utf8PathBuf, InputItem>) -> bool {
        self.init
            || self
                .deps
                .iter()
                .any(|dep| Some(*dep.1) != inputs.get(dep.0).map(|item| item.hash))
    }
}

#[derive(Debug)]
pub(crate) struct Scheduler<'a, D>
where
    D: Send + Sync,
{
    context: &'a Context<D>,
    builder: Arc<RwLock<Builder>>,
    tracked: Vec<Trace<D>>,
    items: HashMap<Utf8PathBuf, InputItem>,
}

impl<'a, D: Send + Sync> Scheduler<'a, D> {
    pub fn new(website: &'a Website<D>, context: &'a Context<D>, items: Vec<InputItem>) -> Self {
        Self {
            context,
            builder: Arc::new(RwLock::new(Builder::new())),
            tracked: website.tasks.iter().cloned().map(Trace::new).collect(),
            items: HashMap::from_iter(items.into_iter().map(|item| (item.file.clone(), item))),
        }
    }

    pub fn build(&mut self) -> Result<(), BuilderError> {
        self.tracked = mem::take(&mut self.tracked)
            .into_par_iter()
            .map(|trace| self.rebuild_trace(trace))
            .collect::<Result<_, _>>()?;
        Ok(())
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
                Url::builder(opts.join(path).parent().unwrap().to_string())
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

        let tracked = Rc::new(RefCell::new(HashMap::new()));

        let pages = trace.task.run(Sack {
            context: self.context,
            builder: self.builder.clone(),
            tracked: tracked.clone(),
            items: &self.items,
        })?;

        for (path, data) in &pages {
            let path = Utf8Path::new("dist").join(path);
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)
                    .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
            }
            let mut file = fs::File::create(&path)
                .map_err(|e| BuilderError::FileWriteError(path.to_owned(), e))?;
            file.write_all(data.as_bytes())
                .map_err(|e| BuilderError::FileWriteError(path.to_owned(), e))?;
            println!("HTML: {}", path);
        }

        let deps = Rc::unwrap_or_clone(tracked).into_inner();
        let path = pages.into_iter().map(|x| x.0).collect();

        Ok(trace.new_with(deps, path))
    }
}
