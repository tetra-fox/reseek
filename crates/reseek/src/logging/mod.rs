//! Logging: the console, the daily log files and the in-memory buffer, behind one filter that
//! `logging.level` and `logging.targets` change while running. `RUST_LOG`, when set, replaces
//! the filter the options give.

mod buffer;
mod file;
mod format;

use std::path::PathBuf;
use std::sync::Arc;

use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::fmt::format::{DefaultFields, Writer};
use tracing_subscriber::fmt::{FormatFields, Layer as FmtLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry, reload};

pub use buffer::{BufferLayer, CAPACITY, LogBuffer};
pub use file::DailyFile;

use crate::config::Options;
use format::{Clock, Line};

/// The name of the span every event of the process is in, which carries the instance name,
/// the invocation id and the process id. The text sinks leave it out of each line.
pub const PROCESS_SPAN: &str = "process";

/// The message a caught panic carries, which is a text unless it was raised with some other
/// value.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
  payload
    .downcast_ref::<&str>()
    .copied()
    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
    .unwrap_or("a panic without a message")
}

/// How many lines may wait for the log file writer before new ones are dropped.
const FILE_QUEUE: usize = 128_000;

/// The installed logging: the filter that can be changed and the buffer the live log view
/// reads.
pub struct Logging {
  filter: reload::Handle<EnvFilter, Registry>,
  buffer: Arc<LogBuffer>,
  /// whether `RUST_LOG` set the filter, which the options then do not change
  from_env: bool,
  /// flushes the log file when the process ends
  _file: Option<WorkerGuard>,
}

/// Where the logs go besides the console.
pub struct Sinks {
  /// the log directory, `None` for no log files
  pub files: Option<PathBuf>,
  /// whether `NO_COLOR` is set to anything
  pub no_color_env: bool,
}

/// The field formatter of the console, a type of its own so a span's fields formatted with
/// colour for the console are not reused by the log file.
struct ConsoleFields(DefaultFields);

impl<'writer> FormatFields<'writer> for ConsoleFields {
  fn format_fields<R: tracing_subscriber::field::RecordFields>(
    &self,
    writer: Writer<'writer>,
    fields: R,
  ) -> std::fmt::Result {
    self.0.format_fields(writer, fields)
  }
}

/// Installs the global subscriber for `options`.
pub fn init(options: &Options, sinks: Sinks) -> Result<Logging, String> {
  let from_env = std::env::var_os("RUST_LOG").is_some();
  let filter = if from_env {
    EnvFilter::try_from_default_env().map_err(|e| format!("RUST_LOG: {e}"))?
  } else {
    filter(options)?
  };
  let (filter, handle) = reload::Layer::new(filter);
  let colour = !(options.logger.no_color || sinks.no_color_env);
  let console = FmtLayer::default()
    .fmt_fields(ConsoleFields(DefaultFields::new()))
    .event_format(Line {
      clock: Clock::Time,
      module: options.debug,
      colour,
    })
    .with_ansi(colour)
    .with_writer(std::io::stdout);
  let (file, guard) = match sinks.files {
    Some(dir) => {
      let writer = DailyFile::new(dir, options.retention.logs);
      let (writer, guard) = NonBlockingBuilder::default()
        .lossy(true)
        .buffered_lines_limit(FILE_QUEUE)
        .finish(writer);
      let layer = FmtLayer::default()
        .event_format(Line {
          clock: Clock::DateTime,
          module: options.debug,
          colour: false,
        })
        .with_ansi(false)
        .with_writer(writer);
      (Some(layer), Some(guard))
    }
    None => (None, None),
  };
  let buffer = Arc::new(LogBuffer::default());
  tracing_subscriber::registry()
    .with(filter)
    .with(console)
    .with(file)
    .with(BufferLayer(buffer.clone()))
    .try_init()
    .map_err(|e| e.to_string())?;
  Ok(Logging {
    filter: handle,
    buffer,
    from_env,
    _file: guard,
  })
}

impl Logging {
  /// Applies `logging.level` and `logging.targets` from `options`; nothing when `RUST_LOG`
  /// set the filter.
  pub fn apply(&self, options: &Options) {
    if self.from_env {
      return;
    }
    match filter(options) {
      Ok(filter) => {
        if let Err(e) = self.filter.reload(filter) {
          tracing::error!(error = %e, "log filter not changed");
        } else {
          tracing::debug!(level = options.logging.level, targets = ?options.logging.targets, "log filter changed");
        }
      }
      Err(e) => tracing::warn!(error = %e, "log filter not changed"),
    }
  }

  /// Whether `RUST_LOG` set the filter.
  pub fn from_env(&self) -> bool {
    self.from_env
  }

  /// The last log records and the stream of new ones.
  pub fn buffer(&self) -> &Arc<LogBuffer> {
    &self.buffer
  }
}

/// The filter for `options`: `logging.level`, at least `debug` in debug mode, then each of
/// `logging.targets`.
pub fn filter(options: &Options) -> Result<EnvFilter, String> {
  EnvFilter::builder()
    .parse(directives(options))
    .map_err(|e| e.to_string())
}

fn directives(options: &Options) -> String {
  const VERBOSITY: [&str; 5] = ["error", "warn", "info", "debug", "trace"];
  let level = options.logging.level.to_ascii_lowercase();
  let rank = |l: &str| VERBOSITY.iter().position(|v| *v == l);
  let level = match (options.debug, rank(&level), rank("debug")) {
    (true, Some(r), Some(debug)) if r < debug => "debug".to_string(),
    _ => level,
  };
  std::iter::once(level)
    .chain(options.logging.targets.iter().cloned())
    .collect::<Vec<_>>()
    .join(",")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_filter_is_the_level_lowered_by_debug_then_the_targets() {
    let mut options = Options::default();
    assert_eq!(directives(&options), "info");
    options.debug = true;
    assert_eq!(directives(&options), "debug");
    options.logging.level = "TRACE".into();
    options.logging.targets = vec!["hyper=warn".into(), "reseek::http=trace".into()];
    assert_eq!(directives(&options), "trace,hyper=warn,reseek::http=trace");
    assert!(filter(&options).is_ok());
  }
}
