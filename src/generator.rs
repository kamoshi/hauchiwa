use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};

use crate::builder::{Input, InputItem};
use crate::error::HauchiwaError;
use crate::{Builder, Context, Hash32, QueryContent};

/// This struct allows for querying the website hierarchy. It is passed to each rendered website
/// page, so that it can easily access the website metadata.
pub struct Sack<'a, G>
where
    G: Send + Sync,
{
    /// Global `Context` for the current build.
    pub(crate) context: &'a Context<G>,
    /// Builder allows scheduling build requests.
    pub(crate) builder: Arc<RwLock<Builder>>,
    /// Tracked dependencies for current instantation.
    pub(crate) tracked: Rc<RefCell<HashMap<Utf8PathBuf, Hash32>>>,
    /// Every single input.
    pub(crate) items: &'a HashMap<Utf8PathBuf, InputItem>,
}

impl<'a, G> Sack<'a, G>
where
    G: Send + Sync,
{
    /// Retrieve global context
    pub fn get_metadata(&self) -> &Context<G> {
        self.context
    }

    pub fn get_content<D>(&self, pattern: &str) -> Result<QueryContent<'_, D>, HauchiwaError>
    where
        D: 'static,
    {
        let glob = glob::Pattern::new(pattern)?;
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

            self.tracked
                .borrow_mut()
                .insert(item.file.clone(), item.hash.clone());

            Ok(QueryContent {
                file: &item.file,
                slug: &item.slug,
                area,
                meta,
                content,
            })
        } else {
            Err(HauchiwaError::AssetNotFound(glob.to_string()))
        }
    }

    /// Retrieve many possible content items.
    pub fn query_content<D>(&self, pattern: &str) -> Result<Vec<QueryContent<'_, D>>, HauchiwaError>
    where
        D: 'static,
    {
        let pattern = glob::Pattern::new(pattern)?;
        let inputs: Vec<_> = self
            .items
            .values()
            .filter(|item| pattern.matches_path(item.slug.as_ref()))
            .collect();

        let mut tracked = self.tracked.borrow_mut();
        for input in inputs.iter() {
            tracked.insert(input.file.clone(), input.hash);
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
            let res = self
                .builder
                .read()
                .map_err(|_| HauchiwaError::LockRead)?
                .check(item.hash);
            if let Some(res) = res {
                return Ok(res);
            }

            let res = self
                .builder
                .write()
                .map_err(|_| HauchiwaError::LockWrite)?
                .build_style(item.hash, style)?;

            self.tracked
                .borrow_mut()
                .insert(item.file.clone(), item.hash);

            Ok(res)
        } else {
            Err(HauchiwaError::AssetNotFound(path.to_string()))
        }
    }

    /// Get optimized image path by original path.
    pub fn get_picture(&self, path: &Utf8Path) -> Result<Utf8PathBuf, HauchiwaError> {
        let input = self
            .items
            .values()
            .find(|item| item.file == path)
            .ok_or_else(|| HauchiwaError::AssetNotFound(path.to_string()))?;

        if let Input::Picture = &input.data {
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
                .build_image(input.hash, &input.file)?;

            self.tracked
                .borrow_mut()
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
                .build_script(input.hash, &input.file)?;

            self.tracked
                .borrow_mut()
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
                Input::Asset(any) => {
                    let data = any.downcast_ref::<T>()?;
                    let file = item.file.clone();
                    let hash = item.hash.clone();
                    Some((data, file, hash))
                }
                _ => None,
            });

        if let Some((data, file, hash)) = found {
            self.tracked.borrow_mut().insert(file, hash);
            return Ok(Some(data));
        }

        Ok(None)
    }
}

pub(crate) fn load_scripts(entrypoints: &HashMap<&str, &str>) -> Vec<InputItem> {
    let mut cmd = Command::new("esbuild");

    for (alias, path) in entrypoints.iter() {
        cmd.arg(format!("{}={}", alias, path));
    }

    let path_scripts = Utf8Path::new(".cache/scripts/");

    let res = cmd
        .arg("--format=esm")
        .arg("--bundle")
        .arg("--minify")
        .arg(format!("--outdir={}", path_scripts))
        .output()
        .unwrap();

    let stderr = String::from_utf8(res.stderr).unwrap();
    println!("{}", stderr);

    entrypoints
        .keys()
        .map(|key| {
            let file = path_scripts.join(key).with_extension("js");
            let buffer = fs::read(&file).unwrap();
            let hash = Sha256::digest(buffer).into();

            InputItem {
                slug: file.clone(),
                file,
                hash,
                data: Input::Script,
            }
        })
        .collect()
}
