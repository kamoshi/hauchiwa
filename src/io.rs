use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Instant;

use camino::Utf8Path;
use console::Style;
use indicatif::{ProgressBar, ProgressStyle};

use crate::HauchiwaError;
use crate::error::ClearError;

const ANSI_BLUE: Style = Style::new().blue();

static PROGRESS_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos} {msg}")
        .expect("Error setting progress bar template")
        .progress_chars("#>-")
});

pub fn as_overhead(s: Instant) -> impl Display {
    let e = Instant::now();
    let f = format!("(+{}ms)", e.duration_since(s).as_millis());
    ANSI_BLUE.apply_to(f)
}

/// Delete the entire `dist` directory if it exists.
pub fn clear_dist() -> Result<(), ClearError> {
    let s = Instant::now();

    if fs::metadata("dist").is_ok() {
        fs::remove_dir_all("dist") //
            .map_err(ClearError::RemoveError)?;
    }

    fs::create_dir("dist") //
        .map_err(ClearError::CreateError)?;

    eprintln!("Cleaned the dist directory {}", as_overhead(s));

    Ok(())
}

pub fn clone_static() -> Result<(), HauchiwaError> {
    let pb = ProgressBar::no_length();
    pb.set_message("Copying static files...");
    pb.set_style(PROGRESS_STYLE.clone());

    let s = Instant::now();
    copy_rec(Utf8Path::new("public"), Utf8Path::new("dist"), &pb)
        .map_err(HauchiwaError::CloneStatic)?;

    pb.finish_with_message(format!("Finished copying static files! {}", as_overhead(s)));

    Ok(())
}

fn copy_rec(src: impl AsRef<Path>, dst: impl AsRef<Path>, pb: &ProgressBar) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let filetype = entry.file_type()?;
        if filetype.is_dir() {
            copy_rec(entry.path(), dst.as_ref().join(entry.file_name()), pb)?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
            pb.inc(1);
        }
    }
    Ok(())
}
