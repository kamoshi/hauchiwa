use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Instant;

use console::Style;
use indicatif::ProgressStyle;
use tracing::{Level, info, span};
use tracing_indicatif::{IndicatifLayer, span_ext::IndicatifSpanExt};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::error::{StepClearError, StepCopyStatic};

const ANSI_BLUE: Style = Style::new().blue();

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

pub fn init_logging() -> Result<(), tracing_subscriber::util::TryInitError> {
    let indicatif_layer = IndicatifLayer::new();

    // Default to INFO, but allow RUST_LOG to override
    let filter = EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(indicatif_layer.get_stderr_writer())
                .with_timer(tracing_subscriber::fmt::time::uptime())
                // Hides the module path like hauchiwa::utils
                .with_target(false)
                .compact(),
        )
        .with(indicatif_layer)
        .with(filter)
        .try_init()
}

pub fn as_overhead(s: Instant) -> impl Display {
    let e = Instant::now();
    let f = format!("(+{}ms)", e.duration_since(s).as_millis());
    ANSI_BLUE.apply_to(f)
}

/// Delete the entire `dist` directory if it exists.
pub fn clear_dist() -> Result<(), StepClearError> {
    let s = Instant::now();

    if fs::metadata("dist").is_ok() {
        fs::remove_dir_all("dist")?;
    }

    fs::create_dir("dist")?;
    info!("Cleaned the dist directory {}", as_overhead(s));

    Ok(())
}

pub fn clone_static(copied: &[(String, String)]) -> Result<(), StepCopyStatic> {
    if copied.is_empty() {
        return Ok(());
    }

    let span = span!(Level::INFO, "copy_static", indicatif.pb_show = true);
    span.pb_set_message("Copying static files...");
    span.pb_set_style(&PROGRESS_STYLE);
    let _enter = span.enter();

    let s = Instant::now();

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
        
        // If the source directory doesn't exist, ignore it or let it fail naturally.
        // It's usually fine to let read_dir return an error if `from` is missing, 
        // but for usability we can just let `copy_rec` handle it.
        if fs::metadata(from).is_ok() {
            copy_rec(from, target, &span)?;
        }
    }

    info!("Finished copying static files! {}", as_overhead(s));

    Ok(())
}

fn copy_rec(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
    span: &tracing::Span,
) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let filetype = entry.file_type()?;
        if filetype.is_dir() {
            copy_rec(entry.path(), dst.as_ref().join(entry.file_name()), span)?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
            span.pb_inc(1);
        }
    }
    Ok(())
}
