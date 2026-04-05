use std::fmt;

use tracing::field::Field;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::field::{RecordFields, Visit};
use tracing_subscriber::fmt::format::{FormatFields, Writer};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Initializes the global tracing subscriber with sensible defaults.
///
/// Sets up a pretty, compact log format with:
/// - ANSI color support
/// - Progress bars via `tracing_indicatif`
/// - Log level controlled by `RUST_LOG` (defaults to `INFO`)
/// - Internal `indicatif.*` span fields hidden from log output
///
/// Returns an error if a global subscriber has already been registered.
pub fn init_logging() -> Result<(), tracing_subscriber::util::TryInitError> {
    let indicatif_layer = IndicatifLayer::new();
    let filter = EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::registry()
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

/// A [`FormatFields`] implementation that skips `indicatif.*` span fields,
/// preventing internal progress-bar metadata from appearing in log lines.
struct CleanFields;

impl<'w> FormatFields<'w> for CleanFields {
    fn format_fields<R: RecordFields>(&self, mut writer: Writer<'w>, fields: R) -> fmt::Result {
        let mut v = CleanVisitor { writer: &mut writer, first: true };
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
        if field.name() == "message" {
            let _ = write!(self.writer, "{val:?}");
        } else {
            let sep = if self.first { "" } else { " " };
            let _ = write!(self.writer, "{sep}{}={val:?}", field.name());
            self.first = false;
        }
    }

    fn record_str(&mut self, field: &Field, val: &str) {
        if field.name().starts_with("indicatif.") {
            return;
        }
        if field.name() == "message" {
            let _ = write!(self.writer, "{val}");
        } else {
            let sep = if self.first { "" } else { " " };
            let _ = write!(self.writer, "{sep}{}={val:?}", field.name());
            self.first = false;
        }
    }
}
