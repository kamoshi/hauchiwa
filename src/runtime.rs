#[cfg(feature = "images")]
mod image;

use std::any::{TypeId, type_name};
use std::fs;
use std::ops::Deref;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};

use crate::loader::assets::BookkeepingDeferred;
use crate::{GitInfo, Globals, Hash32, InputItem};
use crate::{Input, error::*};

const GLOB_OPTS: glob::MatchOptions = glob::MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: true,
};

#[derive(Debug)]
pub struct ViewPage<'a, D> {
    pub file: &'a Utf8Path,
    pub slug: &'a Utf8Path,
    pub area: &'a Utf8Path,
    pub meta: &'a D,
    pub info: Option<&'a GitInfo>,
    pub content: &'a str,
}

/// A simple wrapper for all context data passed at runtime to tasks defined for
/// the website. Use this struct's methods to query required data.
pub struct Context<'a, G>
where
    G: Send + Sync,
{
    /// Global data for the current build.
    globals: &'a Globals<G>,
    /// Every single input.
    items: &'a Vec<&'a InputItem>,
}

impl<'a, G> Context<'a, G>
where
    G: Send + Sync,
{
    pub(crate) fn new(globals: &'a Globals<G>, items: &'a Vec<&'a InputItem>) -> Self {
        Self { globals, items }
    }

    /// Retrieve the globals.
    pub fn get_globals(&self) -> &Globals<G> {
        self.globals
    }

    /// Get the JS script which enables live reloading.
    pub fn get_refresh_script(&self) -> Option<String> {
        self.globals.port.map(|port| {
            format!(
                r#"
const socket = new WebSocket("ws://localhost:{port}");
socket.addEventListener("message", event => {{
    window.location.reload();
}});
"#
            )
        })
    }

    /// Retrieve a single page by glob pattern and metadata shape.
    pub fn glob_page<D>(&self, pattern: &str) -> Result<ViewPage<'_, D>, HauchiwaError>
    where
        D: 'static,
    {
        let glob = glob::Pattern::new(pattern)?;

        let item = self
            .items
            .iter()
            .find(|item| glob.matches_path_with(item.slug.as_ref(), GLOB_OPTS))
            .ok_or_else(|| HauchiwaError::AssetNotFound(glob.to_string()))?;

        if let Input::Content(content) = &item.data {
            let meta = content
                .meta
                .downcast_ref::<D>()
                .ok_or_else(|| HauchiwaError::Frontmatter(item.file.to_string()))?;
            let area = content.area.as_ref();
            let content = content.text.as_str();

            Ok(ViewPage {
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
    pub fn glob_pages<D>(&self, pattern: &str) -> Result<Vec<ViewPage<'_, D>>, HauchiwaError>
    where
        D: 'static,
    {
        let pattern = glob::Pattern::new(pattern)?;

        let inputs: Vec<_> = self
            .items
            .iter()
            .filter(|item| pattern.matches_path(item.slug.as_ref()))
            .collect();

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

                Some(ViewPage {
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

    /// Find the first on‐disk asset whose path matches `pattern`. This asset
    /// will be built only on request and cached.
    pub fn glob<T: 'static>(&self, pattern: &str) -> Result<Option<&T>, HauchiwaError> {
        let refl_type = TypeId::of::<T>();
        let glob = glob::Pattern::new(pattern)?;
        let next = self
            .items
            .iter()
            .filter(|item| {
                item.refl_type == refl_type
                    && glob.matches_path_with(item.file.as_std_path(), GLOB_OPTS)
            })
            .map(Deref::deref)
            .next();

        Ok(match next {
            Some(item) => match &item.data {
                Input::Content(content) => content.meta.downcast_ref(),
                Input::Just(just) => just.downcast_ref(),
                Input::Lazy(lazy) => lazy.downcast_ref(),
            },
            None => None,
        })
    }

    pub fn get<T: 'static>(&self, path: &str) -> Result<&T, HauchiwaError> {
        let item = self.find_item_by_path(path)?;

        let data = match &item.data {
            Input::Content(content) => content.meta.downcast_ref(),
            Input::Just(just) => just.downcast_ref(),
            Input::Lazy(lazy) => lazy.downcast_ref(),
        };

        match data {
            Some(data) => Ok(data),
            None => {
                let have = item.refl_name;
                let need = type_name::<T>();
                eprintln!("Requested {need}, but received {have}");
                todo!()
            }
        }
    }

    fn find_item_by_path(&self, path: &str) -> Result<&InputItem, HauchiwaError> {
        self.items
            .iter()
            .map(Deref::deref)
            .find(|item| item.file == path)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))
    }
}

pub fn build_image(hash: Hash32, file: &Utf8Path) -> Result<Utf8PathBuf, HauchiwaError> {
    let hash = hash.to_hex();
    let path_root = Utf8Path::new("/hash/img/").join(&hash);
    let path_hash = Utf8Path::new(".cache/hash/img/").join(&hash);
    let path_dist = Utf8Path::new("dist/hash/img/").join(&hash);

    // If this hash exists it means the work is already done.
    if !path_hash.exists() {
        let buffer = fs::read(file) //
            .map_err(|e| BuilderError::FileReadError(file.to_path_buf(), e))?;
        let buffer = image::process_image(&buffer);

        fs::create_dir_all(".cache/hash/img/")
            .map_err(|e| BuilderError::CreateDirError(".cache/hash".into(), e))?;
        fs::write(&path_hash, buffer).unwrap();
    }

    let dir = path_dist.parent().unwrap_or(&path_dist);
    fs::create_dir_all(dir) //
        .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
    fs::copy(&path_hash, &path_dist)
        .map_err(|e| BuilderError::FileCopyError(path_hash.to_owned(), path_dist.clone(), e))?;

    Ok(path_root)
}

pub fn build_deferred(
    hash: Hash32,
    path_file: &Utf8Path,
    bookkeeping: Arc<BookkeepingDeferred>,
) -> Result<Utf8PathBuf, BuilderError> {
    // We can check here whether the given file was already built, and we
    // know this, because we keep the original file's content hash, as well
    // as the resulting file's content hash.
    if let Some(hash) = bookkeeping.read(hash) {
        let path_root = Utf8Path::new("/hash/").join(hash.to_hex());
        return Ok(path_root);
    };

    // If the hash was not saved previously, we proceed normally, build the
    // artifact and then save the hash of the result.
    let buffer = fs::read(path_file) //
        .map_err(|e| BuilderError::FileReadError(path_file.to_path_buf(), e))?;
    let result = (bookkeeping.func)(&buffer);
    let result_hash = Hash32::hash(&result);

    bookkeeping.save(hash, result_hash);
    let path = write_hashed_data(&buffer, result_hash, "")?;

    Ok(path)
}

pub fn write_hashed_data(
    data: &[u8],
    hash: Hash32,
    ext: &str,
) -> Result<Utf8PathBuf, BuilderError> {
    let hash = hash.to_hex();

    let path_temp = Utf8Path::new(".cache/hash").join(&hash);
    let path_dist = Utf8Path::new("dist/hash").join(&hash).with_extension(ext);
    let path_root = Utf8Path::new("/hash/").join(&hash).with_extension(ext);

    if !path_temp.exists() {
        fs::create_dir_all(".cache/hash")
            .map_err(|e| BuilderError::CreateDirError(".cache/hash".into(), e))?;
        fs::write(&path_temp, data)
            .map_err(|e| BuilderError::FileWriteError(path_temp.clone(), e))?;
    }

    let dir = path_dist.parent().unwrap_or(&path_dist);
    fs::create_dir_all(dir) //
        .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
    fs::copy(&path_temp, &path_dist)
        .map_err(|e| BuilderError::FileCopyError(path_temp.clone(), path_dist.clone(), e))?;

    Ok(path_root)
}
