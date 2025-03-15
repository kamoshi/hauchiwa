use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::{fs, mem};

use camino::{Utf8Path, Utf8PathBuf};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use sha2::{Digest, Sha256};
use sitemap_rs::url::{ChangeFrequency, Url};
use sitemap_rs::url_set::UrlSet;

use crate::error::LoaderError;
use crate::generator::{Sack, Tracker};
use crate::{Builder, BuilderError, Context, Hash32, HauchiwaError, Task, Website};

/// Init pointer used to dynamically retrieve front matter. The type of front matter
/// needs to be erased at run time and this is one way of accomplishing this,
/// it's hidden behind the `dyn Fn` existential type.
type InitFnPtr = Arc<dyn Fn(&str) -> Result<(Arc<dyn Any + Send + Sync>, String), LoaderError>>;

/// Wraps `InitFnPtr` and implements `Debug` trait for function pointer.
#[derive(Clone)]
pub(crate) struct InitFn(pub(crate) InitFnPtr);

impl InitFn {
    /// Call the contained `InitFn` pointer.
    pub(crate) fn call(
        &self,
        data: &str,
    ) -> Result<(Arc<dyn Any + Send + Sync>, String), LoaderError> {
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
pub struct Trace<D>
where
    D: Send + Sync,
{
    task: Task<D>,
    init: bool,
    deps: HashMap<Utf8PathBuf, Hash32>,
    glob: Vec<glob::Pattern>,
    pub(crate) path: Vec<(Utf8PathBuf, String)>,
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
pub(crate) struct Scheduler<'a, D>
where
    D: Send + Sync,
{
    context: &'a Context<D>,
    builder: Arc<RwLock<Builder>>,
    pub(crate) tracked: Vec<Trace<D>>,
    items: HashMap<Utf8PathBuf, InputItem>,
    cache_pages: HashMap<Utf8PathBuf, Hash32>,
}

impl<'a, D: Send + Sync> Scheduler<'a, D> {
    pub fn new(website: &'a Website<D>, context: &'a Context<D>, items: Vec<InputItem>) -> Self {
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

        let paths = trace.task.run(Sack {
            context: self.context,
            builder: self.builder.clone(),
            tracker: tracker.clone(),
            items: &self.items,
        })?;

        let tracker = Rc::unwrap_or_clone(tracker).into_inner();

        Ok(trace.new_with(tracker, paths))
    }

    pub(crate) fn remove(&mut self, paths: HashSet<&Path>) {
        self.items = mem::take(&mut self.items)
            .into_iter()
            .filter(|p| !paths.contains(p.1.file.as_std_path()))
            .collect();
    }

    pub(crate) fn refresh(&mut self) -> Result<(), HauchiwaError> {
        self.build_pages()?;
        self.write_pages()?;

        Ok(())
    }

    fn build_pages(&mut self) -> Result<(), BuilderError> {
        self.tracked = mem::take(&mut self.tracked)
            .into_par_iter()
            .map(|trace| self.rebuild_trace(trace))
            .collect::<Result<_, _>>()?;
        Ok(())
    }

    fn write_pages(&mut self) -> Result<(), HauchiwaError> {
        let mut temp = HashMap::new();

        for trace in &self.tracked {
            for (slug, data) in &trace.path {
                let hash = Sha256::digest(&data).into();
                let path = Utf8Path::new("dist").join(slug);

                // if path.as_str().contains("test") {
                //     println!("{}", &data);
                // }

                if Some(hash) == self.cache_pages.get(&path).copied() {
                    continue;
                } else {
                    self.cache_pages.insert(path.clone(), hash);
                }

                if temp.contains_key(&path) {
                    println!("Warning, overwriting path {slug}")
                }

                if let Some(dir) = path.parent() {
                    fs::create_dir_all(dir)
                        .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
                }
                let mut file = fs::File::create(&path)
                    .map_err(|e| BuilderError::FileWriteError(path.to_owned(), e))?;
                std::io::Write::write_all(&mut file, data.as_bytes())
                    .map_err(|e| BuilderError::FileWriteError(path.to_owned(), e))?;

                println!("HTML: {}", path);

                temp.insert(path.clone(), hash);
            }
        }

        self.cache_pages.extend(temp.into_iter());

        Ok(())
    }
}
