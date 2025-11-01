use camino::{Utf8Path, Utf8PathBuf};

pub fn normalize_prefixed(prefix: &str, path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
    let path = path.as_ref().strip_prefix(prefix).unwrap_or(path.as_ref());

    normalize(path)
}

pub fn normalize(path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
    let mut buffer = path.as_ref().to_path_buf();

    if let Some(file_name) = buffer.file_name() {
        if file_name == "index" || file_name.starts_with("index.") {
            buffer.set_extension("html");
        } else {
            buffer.set_extension("");
            buffer.push("index.html");
        }
    } else {
        buffer.push("index.html");
    }

    buffer
}

pub fn absolutize(prefix: &str, path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
    let path = path.as_ref().strip_prefix(prefix).unwrap_or(path.as_ref());
    let path = Utf8Path::new("/").join(path);

    if let Some(file_name) = path.file_name() {
        if file_name == "index" || file_name.starts_with("index.") {
            path.parent().unwrap().to_path_buf()
        } else {
            path.with_extension("")
        }
    } else {
        path
    }
}

#[derive(Debug, Clone)]
pub struct Page {
    pub url: Utf8PathBuf,
    pub content: String,
}

impl Page {
    pub fn html(path: impl AsRef<Utf8Path>, content: impl Into<String>) -> Self {
        Self {
            url: normalize(path),
            content: content.into(),
        }
    }

    pub fn file(path: impl Into<Utf8PathBuf>, content: impl Into<String>) -> Self {
        Self {
            url: path.into(),
            content: content.into(),
        }
    }
}

use std::fs;
use std::io;
use std::path::Path;

/// Saves all pages to the "dist" directory.
pub fn save_pages_to_dist(pages: &[Page]) -> io::Result<()> {
    let output_dir = Path::new("dist");

    // 1. Create the "dist/" directory if it doesn't exist.
    //    This does nothing if it already exists.
    fs::create_dir_all(output_dir)?;

    for page in pages {
        // 2. Create the full path for the file.
        //    e.g., "dist" + "blog/my-post.html" = "dist/blog/my-post.html"
        let file_path = output_dir.join(&page.url);

        // 3. IMPORTANT: Ensure the file's parent directory exists.
        //    If file_path is "dist/blog/my-post.html", this creates "dist/blog/".
        if let Some(parent_dir) = file_path.parent() {
            fs::create_dir_all(parent_dir)?;
        }

        // 4. Write (or overwrite) the file.
        //    This handles your "overwrite existing or make new" logic.
        fs::write(&file_path, &page.content)?;

        println!(
            "Saved: {} ({} bytes)",
            file_path.display(),
            page.content.len()
        );
    }

    Ok(())
}
