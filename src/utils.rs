use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use console::Style;
use indicatif::ProgressStyle;
use tracing::{Level, info, span};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::error::StepCopyStatic;

const ANSI_BLUE: Style = Style::new().blue();

#[allow(clippy::expect_used)] // hardcoded template literal — cannot fail
static PROGRESS_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed}] [{bar:40.cyan/blue}] {pos} {msg}")
        .expect("Error setting progress bar template")
        .progress_chars("#>-")
});

pub fn get_style_task() -> Result<ProgressStyle, indicatif::style::TemplateError> {
    ProgressStyle::default_spinner().template("{spinner:.blue} {msg}")
}

pub fn get_style_task_progress() -> Result<ProgressStyle, indicatif::style::TemplateError> {
    ProgressStyle::default_spinner().template("{spinner:.blue} {msg} {pos}/{len} ")
}

pub fn as_overhead(s: Instant) -> impl Display {
    let e = Instant::now();
    let f = format!("(+{}ms)", e.duration_since(s).as_millis());
    ANSI_BLUE.apply_to(f)
}


/// Copies all static file trees configured via `Blueprint::copy_static` into `dist/`.
///
/// Returns the list of `(source_path, dist_relative_path)` pairs for every file
/// that was copied. These are inserted into the [`Snapshot`](crate::output::Snapshot)
/// by the caller so that step 4 can reconcile `dist` without `clear_dist()`.
pub fn clone_static(
    copied: &[(String, String)],
) -> Result<Vec<(Utf8PathBuf, Utf8PathBuf)>, StepCopyStatic> {
    if copied.is_empty() {
        return Ok(vec![]);
    }

    let span = span!(Level::INFO, "copy_static", indicatif.pb_show = true);
    span.pb_set_message("Copying static files...");
    span.pb_set_style(&PROGRESS_STYLE);
    let _enter = span.enter();

    let s = Instant::now();
    let mut entries = Vec::new();

    for (into, from) in copied {
        let path = std::path::Path::new(into);
        let mut depth = 0;
        let mut safe = true;

        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    depth -= 1;
                    if depth < 0 {
                        safe = false;
                        break;
                    }
                }
                std::path::Component::Normal(_) => {
                    depth += 1;
                }
                std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                    safe = false;
                    break;
                }
                std::path::Component::CurDir => {}
            }
        }

        if !safe {
            return Err(StepCopyStatic::UnsafeTarget(into.clone()));
        }

        let target = std::path::Path::new("dist").join(into);
        let dist_rel = Utf8Path::new(into);

        if fs::metadata(from).is_ok() {
            copy_rec(from, target, dist_rel, &span, &mut entries)?;
        }
    }

    info!("Finished copying static files! {}", as_overhead(s));

    Ok(entries)
}

fn copy_rec(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
    dist_rel: &Utf8Path,
    span: &tracing::Span,
    entries: &mut Vec<(Utf8PathBuf, Utf8PathBuf)>,
) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name
            .to_str()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "non-UTF-8 filename"))?;

        if entry.file_type()?.is_dir() {
            copy_rec(
                entry.path(),
                dst.as_ref().join(&name),
                &dist_rel.join(name_str),
                span,
                entries,
            )?;
        } else {
            let src_path = entry.path();
            let dst_file = dst.as_ref().join(&name);
            let unchanged = crate::core::Hash32::hash_file(&src_path)
                .ok()
                .zip(crate::core::Hash32::hash_file(&dst_file).ok())
                .map(|(s, d)| s == d)
                .unwrap_or(false);
            if !unchanged {
                fs::copy(&src_path, &dst_file)?;
            }
            span.pb_inc(1);

            let source = Utf8PathBuf::try_from(src_path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            entries.push((source, dist_rel.join(name_str)));
        }
    }

    Ok(())
}
