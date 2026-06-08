use std::fmt;

use tracing::field::Field;
use tracing::{Event, Level, Subscriber};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::field::{RecordFields, Visit};
use tracing_subscriber::fmt::format::{FormatFields, Writer};
use tracing_subscriber::fmt::{FmtContext, FormatEvent};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// The formatting style for the logging output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogFormat {
    /// A structured, compact format (e.g. `INFO Finished task name="..." duration_ms=...`).
    /// Best for log parsing and automated telemetry.
    #[default]
    Structured,
    /// A clean, human-readable format modeled after cargo (e.g. `Finished content/index.md in 2ms`).
    /// Best for local development and CLI tools.
    Humane,
}

/// Initializes the global tracing subscriber with sensible defaults using structured formatting.
///
/// Sets up a pretty, compact log format with:
/// - ANSI color support
/// - Progress bars via `tracing_indicatif`
/// - Log level controlled by `RUST_LOG` (defaults to `INFO`)
/// - Internal `indicatif.*` span fields hidden from log output
///
/// Returns an error if a global subscriber has already been registered.
pub fn init_logging() -> Result<(), tracing_subscriber::util::TryInitError> {
    init_logging_with_format(LogFormat::Structured)
}

/// Initializes the global tracing subscriber with a specific format.
///
/// Returns an error if a global subscriber has already been registered.
pub fn init_logging_with_format(format: LogFormat) -> Result<(), tracing_subscriber::util::TryInitError> {
    let filter = EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        .from_env_lossy();
    let registry = tracing_subscriber::registry();

    match format {
        LogFormat::Structured => {
            let indicatif_layer = IndicatifLayer::new();
            registry
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(indicatif_layer.get_stderr_writer())
                        .with_timer(tracing_subscriber::fmt::time::uptime())
                        .with_target(false)
                        .with_ansi(true)
                        .fmt_fields(CleanFields)
                        .compact(),
                )
                .with(indicatif_layer)
                .with(filter)
                .try_init()
        }
        LogFormat::Humane => {
            let indicatif_layer = IndicatifLayer::new();
            registry
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(indicatif_layer.get_stderr_writer())
                        .fmt_fields(CleanFields)
                        .event_format(HumaneFormatter),
                )
                .with(indicatif_layer)
                .with(filter)
                .try_init()
        }
    }
}

/// A [`FormatFields`] implementation that skips `indicatif.*` span fields,
/// preventing internal progress-bar metadata from appearing in log lines.
///
/// Use this when composing your own `tracing` subscriber with an
/// [`IndicatifLayer`](tracing_indicatif::IndicatifLayer). Without this
/// formatter, internal fields like `indicatif.pb_show` will leak into
/// your log output.
///
/// # Example
///
/// ```rust,no_run
/// use tracing_subscriber::{Registry, layer::SubscriberExt, util::SubscriberInitExt};
/// use tracing_indicatif::IndicatifLayer;
///
/// let indicatif_layer = IndicatifLayer::new();
/// Registry::default()
///     .with(
///         tracing_subscriber::fmt::layer()
///             .with_writer(indicatif_layer.get_stderr_writer())
///             .fmt_fields(hauchiwa::CleanFields)
///     )
///     .with(indicatif_layer)
///     .init();
/// ```
pub struct CleanFields;

impl<'w> FormatFields<'w> for CleanFields {
    fn format_fields<R: RecordFields>(&self, mut writer: Writer<'w>, fields: R) -> fmt::Result {
        let mut v = CleanVisitor {
            writer: &mut writer,
            first: true,
        };
        fields.record(&mut v);
        Ok(())
    }
}

struct CleanVisitor<'a, 'w> {
    writer: &'a mut Writer<'w>,
    first: bool,
}

impl Visit for CleanVisitor<'_, '_> {
    fn record_debug(&mut self, field: &Field, val: &dyn fmt::Debug) {
        if field.name().starts_with("indicatif.") {
            return;
        }
        let sep = if self.first { "" } else { " " };
        if field.name() == "message" {
            let _ = write!(self.writer, "{sep}{val:?}");
        } else {
            let _ = write!(self.writer, "{sep}{}={val:?}", field.name());
        }
        self.first = false;
    }

    fn record_str(&mut self, field: &Field, val: &str) {
        if field.name().starts_with("indicatif.") {
            return;
        }
        let sep = if self.first { "" } else { " " };
        if field.name() == "message" {
            let _ = write!(self.writer, "{sep}{val}");
        } else {
            let _ = write!(self.writer, "{sep}{}={val:?}", field.name());
        }
        self.first = false;
    }
}

/// A [`FormatEvent`] implementation that renders logs in a clean, human-readable format.
pub struct HumaneFormatter;

impl<S, N> FormatEvent<S, N> for HumaneFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut visitor = HumaneVisitor {
            message: String::new(),
            name: None,
            duration_ms: None,
            url: None,
        };
        event.record(&mut visitor);

        let metadata = event.metadata();
        let level = metadata.level();

        let ansi = writer.has_ansi_escapes();
        let bold_green = if ansi { "\x1b[1;32m" } else { "" };
        let bold_yellow = if ansi { "\x1b[1;33m" } else { "" };
        let bold_red = if ansi { "\x1b[1;31m" } else { "" };
        let reset = if ansi { "\x1b[0m" } else { "" };

        if *level == Level::ERROR {
            writeln!(
                writer,
                "   {}Error:{} {}",
                bold_red, reset, visitor.message
            )?;
            return Ok(());
        }

        if *level == Level::WARN {
            writeln!(
                writer,
                " {}Warning:{} {}",
                bold_yellow, reset, visitor.message
            )?;
            return Ok(());
        }

        if *level == Level::INFO {
            if visitor.message == "Finished task" {
                if let Some(name) = visitor.name {
                    let duration = visitor.duration_ms.unwrap_or(0);
                    writeln!(
                        writer,
                        "    {}Finished{} {} in {}ms",
                        bold_green, reset, name, duration
                    )?;
                    return Ok(());
                }
            }

            if visitor.message == "Finished copying static files" {
                let duration = visitor.duration_ms.unwrap_or(0);
                writeln!(
                    writer,
                    "    {}Finished{} copying static files in {}ms",
                    bold_green, reset, duration
                )?;
                return Ok(());
            }

            if visitor.message == "Build complete!" {
                writeln!(
                    writer,
                    "    {}Finished{} build complete!",
                    bold_green, reset
                )?;
                return Ok(());
            }

            if let Some(url) = visitor.url {
                writeln!(
                    writer,
                    "     {}Serving{} at {}",
                    bold_green, reset, url
                )?;
                return Ok(());
            }

            // Fallback for general info logs
            writeln!(writer, "      {}", visitor.message)?;
            return Ok(());
        }

        // Fallback for debug/trace logs
        writeln!(writer, "      {}", visitor.message)?;
        Ok(())
    }
}

struct HumaneVisitor {
    message: String,
    name: Option<String>,
    duration_ms: Option<u64>,
    url: Option<String>,
}

impl Visit for HumaneVisitor {
    fn record_debug(&mut self, field: &Field, val: &dyn fmt::Debug) {
        let name = field.name();
        if name == "message" {
            self.message = clean_str(&format!("{val:?}"));
        } else if name == "name" {
            self.name = Some(clean_str(&format!("{val:?}")));
        } else if name == "url" {
            self.url = Some(clean_str(&format!("{val:?}")));
        }
    }

    fn record_str(&mut self, field: &Field, val: &str) {
        let name = field.name();
        if name == "message" {
            self.message = val.to_string();
        } else if name == "name" {
            self.name = Some(val.to_string());
        } else if name == "url" {
            self.url = Some(val.to_string());
        }
    }

    fn record_u64(&mut self, field: &Field, val: u64) {
        if field.name() == "duration_ms" {
            self.duration_ms = Some(val);
        }
    }

    fn record_i64(&mut self, field: &Field, val: i64) {
        if field.name() == "duration_ms" && val >= 0 {
            self.duration_ms = Some(val as u64);
        }
    }
}

fn clean_str(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}
