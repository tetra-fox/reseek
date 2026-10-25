//! The line format of the console and the log files: `[time LVL] message fields`, with the
//! module in brackets in debug mode and the enclosing spans before the message.

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
}

/// The line format.
pub struct Line {
  /// how each line starts
  pub clock: Clock,
  /// names the module on every line
  pub module: bool,
  /// colours the level
  pub colour: bool,
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

/// The ANSI colour of a level.
fn colour(level: Level) -> &'static str {
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
    let time = match self.clock {
      Clock::Time => now.format("%H:%M:%S"),
      Clock::DateTime => now.format("%Y-%m-%dT%H:%M:%S%.3f"),
    };
    let level = *event.metadata().level();
    if self.colour {
      write!(
        writer,
        "[{time} {}{}\x1b[0m] ",
        colour(level),
        abbreviation(level)
      )?;
    } else {
      write!(writer, "[{time} {}] ", abbreviation(level))?;
    }
    if self.module {
      write!(writer, "[{}] ", event.metadata().target())?;
    }
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
