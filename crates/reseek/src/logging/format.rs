//! The line format of the console and the log files: `[time LVL] [module] spans: message
//! fields`, the module being the event's target and the spans those enclosing it.

use std::fmt;

use chrono::Local;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields};
use tracing_subscriber::registry::LookupSpan;

/// How each line starts.
#[derive(Clone, Copy)]
pub enum Clock {
  /// `HH:mm:ss`, for the console
  Time,
  /// `yyyy-MM-ddTHH:mm:ss.fff`, for the files
  DateTime,
  /// no time, for a sink that times each line itself
  Off,
}

/// The line format.
pub struct Line {
  /// how each line starts
  pub clock: Clock,
  /// colors the level
  pub color: bool,
}

/// The three-letter name of a level.
fn abbreviation(level: Level) -> &'static str {
  match level {
    Level::TRACE => "VRB",
    Level::DEBUG => "DBG",
    Level::INFO => "INF",
    Level::WARN => "WRN",
    Level::ERROR => "ERR",
  }
}

/// The ANSI color of a level.
fn color(level: Level) -> &'static str {
  match level {
    Level::TRACE => "\x1b[2m",
    Level::DEBUG => "\x1b[36m",
    Level::INFO => "\x1b[32m",
    Level::WARN => "\x1b[33m",
    Level::ERROR => "\x1b[31m",
  }
}

impl<S, N> FormatEvent<S, N> for Line
where
  S: Subscriber + for<'a> LookupSpan<'a>,
  N: for<'a> FormatFields<'a> + 'static,
{
  fn format_event(
    &self,
    ctx: &FmtContext<'_, S, N>,
    mut writer: Writer<'_>,
    event: &Event<'_>,
  ) -> fmt::Result {
    let now = Local::now();
    write!(writer, "[")?;
    match self.clock {
      Clock::Time => write!(writer, "{} ", now.format("%H:%M:%S"))?,
      Clock::DateTime => write!(writer, "{} ", now.format("%Y-%m-%dT%H:%M:%S%.3f"))?,
      Clock::Off => {}
    }
    let level = *event.metadata().level();
    if self.color {
      write!(writer, "{}{}\x1b[0m] ", color(level), abbreviation(level))?;
    } else {
      write!(writer, "{}] ", abbreviation(level))?;
    }
    write!(writer, "[{}] ", event.metadata().target())?;
    if let Some(scope) = ctx.event_scope() {
      for span in scope
        .from_root()
        .filter(|span| span.name() != super::PROCESS_SPAN)
      {
        write!(writer, "{}", span.name())?;
        let extensions = span.extensions();
        if let Some(fields) = extensions.get::<FormattedFields<N>>()
          && !fields.is_empty()
        {
          write!(writer, "{{{fields}}}")?;
        }
        write!(writer, ": ")?;
      }
    }
    ctx.format_fields(writer.by_ref(), event)?;
    writeln!(writer)
  }
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use reseek_shares::lock;
  use tracing_subscriber::fmt::Layer as FmtLayer;
  use tracing_subscriber::layer::SubscriberExt;

  use super::*;

  #[derive(Clone, Default)]
  struct Written(Arc<Mutex<Vec<u8>>>);

  impl std::io::Write for Written {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
      lock(&self.0).extend_from_slice(bytes);
      Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
      Ok(())
    }
  }

  #[test]
  fn a_line_names_its_module_ahead_of_the_spans_without_the_process_span() {
    let written = Written::default();
    let writer = written.clone();
    let layer = FmtLayer::default()
      .event_format(Line {
        clock: Clock::Off,
        color: false,
      })
      .with_ansi(false)
      .with_writer(move || writer.clone());
    tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
      let _process = tracing::info_span!(super::super::PROCESS_SPAN, pid = 1).entered();
      let _search = tracing::info_span!("search", token = 7).entered();
      tracing::info!(found = 2, "results");
    });
    let line = String::from_utf8(lock(&written.0).clone()).unwrap();
    assert_eq!(
      line,
      "[INF] [reseek::logging::format::tests] search{token=7}: results found=2\n"
    );
  }
}
