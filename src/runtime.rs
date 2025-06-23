use std::fs;
use std::sync::{Arc, RwLock};

use camino::{Utf8Path, Utf8PathBuf};

use crate::loader::assets::Bookkeeping;
use crate::{Builder, GitInfo, Globals, Hash32, InputItem};
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
    /// Builder allows scheduling build requests.
    builder: Arc<RwLock<Builder>>,
    /// Every single input.
    items: &'a Vec<&'a InputItem>,
}

impl<'a, G> Context<'a, G>
where
    G: Send + Sync,
{
    pub(crate) fn new(
        globals: &'a Globals<G>,
        builder: Arc<RwLock<Builder>>,
        items: &'a Vec<&'a InputItem>,
    ) -> Self {
        Self {
            globals,
            builder,
            items,
        }
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

    pub fn glob_asset<T>(&self, pattern: &str) -> Result<Option<&T>, HauchiwaError>
    where
        T: 'static,
    {
        let glob = glob::Pattern::new(pattern)?;

        let found = self
            .items
            .iter()
            .filter(|item| glob.matches_path_with(item.file.as_std_path(), GLOB_OPTS))
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
            return Ok(Some(data));
        }

        Ok(None)
    }

    /// Find the first on‐disk asset whose path matches `pattern`. This asset
    /// will be built only on request and cached by hash.
    pub fn glob_asset_deferred(&self, pattern: &str) -> Result<Option<Utf8PathBuf>, HauchiwaError> {
        let glob = glob::Pattern::new(pattern)?;
        let found = self.items.iter().find_map(|item| {
            if !glob.matches_path_with(item.file.as_std_path(), GLOB_OPTS) {
                return None;
            }

            match &item.data {
                Input::OnDisk(bookkeeping) => Some((item, bookkeeping)),
                _ => None,
            }
        });

        let (item, bookkeeping) = match found {
            Some(found) => found,
            None => return Ok(None),
        };

        let path = build_deferred(item.hash, &item.file, bookkeeping.clone())?;
        Ok(Some(path))
    }

    /// Get style by absolute file path
    pub fn get_style(&self, path: &Utf8Path) -> Result<Utf8PathBuf, HauchiwaError> {
        let item = self
            .items
            .iter()
            .find(|item| item.file == path)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))?;

        if let Input::Stylesheet(style) = &item.data {
            let path = self
                .builder
                .read()
                .map_err(|_| HauchiwaError::LockRead)?
                .check(item.hash);

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
            .iter()
            .find(|item| item.file == path)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))?;

        if let Input::OnDisk(bookkeeping) = &input.data {
            let res = build_deferred(input.hash, &input.file, bookkeeping.clone())?;
            Ok(res)
        } else {
            Err(HauchiwaError::AssetNotFound(path.to_string()))
        }
    }

    pub fn get_script(&self, path: &str) -> Result<Utf8PathBuf, HauchiwaError> {
        let input = self
            .items
            .iter()
            .find(|item| item.file == path)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))?;

        if let Input::Script = &input.data {
            let hash = input.hash.to_hex();
            let path_hash = Utf8Path::new(".cache/hash/").join(&hash);
            let path_dist = Utf8Path::new("dist/hash/").join(&hash).with_extension("js");
            let path_root = Utf8Path::new("/hash/").join(&hash).with_extension("js");

            let dir = path_dist.parent().unwrap_or(&path_dist);
            fs::create_dir_all(dir) //
                .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
            fs::copy(&path_hash, &path_dist).map_err(|e| {
                BuilderError::FileCopyError(path_hash.to_owned(), path_dist.clone(), e)
            })?;

            Ok(path_root)
        } else {
            Err(HauchiwaError::AssetNotFound(path.to_string()))
        }
    }
}

fn build_deferred(
    hash: Hash32,
    path_file: &Utf8Path,
    bookkeeping: Arc<Bookkeeping>,
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
    let buffer =
        fs::read(path_file).map_err(|e| BuilderError::FileReadError(path_file.to_path_buf(), e))?;
    let result = (bookkeeping.func)(&buffer);
    let result_hash = Hash32::hash(&result);
    let result_hash_hex = result_hash.to_hex();

    bookkeeping.save(hash, result_hash);

    let path_temp = Utf8Path::new(".cache/hash").join(&result_hash_hex);
    let path_dist = Utf8Path::new("dist/hash").join(&result_hash_hex);
    let path_root = Utf8Path::new("/hash/").join(&result_hash_hex);

    if !path_temp.exists() {
        fs::create_dir_all(".cache/hash")
            .map_err(|e| BuilderError::CreateDirError(".cache/hash".into(), e))?;
        fs::write(&path_temp, buffer)
            .map_err(|e| BuilderError::FileWriteError(path_temp.clone(), e))?;
    }

    let dir = path_dist.parent().unwrap_or(&path_dist);
    fs::create_dir_all(dir) //
        .map_err(|e| BuilderError::CreateDirError(dir.to_owned(), e))?;
    fs::copy(&path_temp, &path_dist)
        .map_err(|e| BuilderError::FileCopyError(path_temp.clone(), path_dist.clone(), e))?;

    Ok(path_root)
}
