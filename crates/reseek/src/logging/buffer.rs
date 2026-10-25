//! The last log records kept in memory, for the live log view, and the stream of new ones.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use api_types::logs::{LogLevel, LogRecord};
use chrono::Utc;
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

/// How many records the buffer keeps.
pub const CAPACITY: usize = 100;

/// How many new records a slow reader may fall behind by before it misses some.
const SUBSCRIBERS_LAG: usize = 1024;

/// The field that names what a module logged on behalf of, such as a share host.
const SUB_CONTEXT: &str = "sub";

/// The last [`CAPACITY`] log records, oldest first, and a stream of each new one.
#[derive(Debug)]
pub struct LogBuffer {
  records: Mutex<VecDeque<Arc<LogRecord>>>,
  new: broadcast::Sender<Arc<LogRecord>>,
}

impl Default for LogBuffer {
  fn default() -> Self {
    Self {
      records: Mutex::new(VecDeque::with_capacity(CAPACITY + 1)),
      new: broadcast::Sender::new(SUBSCRIBERS_LAG),
    }
  }
}

impl LogBuffer {
  /// The records kept, oldest first.
  pub fn snapshot(&self) -> Vec<LogRecord> {
    self
      .lock()
      .iter()
      .map(|record| LogRecord::clone(record))
      .collect()
  }

  /// Every record logged from now on.
  pub fn subscribe(&self) -> broadcast::Receiver<Arc<LogRecord>> {
    self.new.subscribe()
  }

  fn push(&self, record: LogRecord) {
    let record = Arc::new(record);
    {
      let mut records = self.lock();
      records.push_back(record.clone());
      if records.len() > CAPACITY {
        records.pop_front();
      }
    }
    // an error only means nobody is listening
    drop(self.new.send(record));
  }

  fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<Arc<LogRecord>>> {
    // a push is one step, so a panic while holding the lock leaves the buffer whole
    self.records.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

/// The layer that turns every event the filter lets through into a [`LogRecord`] in a
/// [`LogBuffer`].
pub struct BufferLayer(pub Arc<LogBuffer>);

impl<S: Subscriber> Layer<S> for BufferLayer {
  fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
    let mut fields = Fields::default();
    event.record(&mut fields);
    let metadata = event.metadata();
    self.0.push(LogRecord {
      timestamp: Utc::now(),
      context: metadata.target().to_string(),
      sub_context: fields.sub,
      level: level(*metadata.level()),
      message: fields.text + &fields.rest,
    });
  }
}

/// The record's level for a `tracing` level.
pub fn level(level: Level) -> LogLevel {
  match level {
    Level::TRACE => LogLevel::Verbose,
    Level::DEBUG => LogLevel::Debug,
    Level::INFO => LogLevel::Information,
    Level::WARN => LogLevel::Warning,
    Level::ERROR => LogLevel::Error,
  }
}

/// An event's message followed by its other fields as `name=value`.
#[derive(Default)]
struct Fields {
  text: String,
  rest: String,
  sub: Option<String>,
}

impl Visit for Fields {
  fn record_str(&mut self, field: &Field, value: &str) {
    match field.name() {
      "message" => self.text.push_str(value),
      SUB_CONTEXT => self.sub = Some(value.to_string()),
      name => self.rest.push_str(&format!(" {name}={value}")),
    }
  }

  fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
    match field.name() {
      "message" => self.text.push_str(&format!("{value:?}")),
      SUB_CONTEXT => self.sub = Some(format!("{value:?}")),
      name => self.rest.push_str(&format!(" {name}={value:?}")),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tracing_subscriber::layer::SubscriberExt;

  #[test]
  fn the_buffer_keeps_the_last_hundred_records_with_their_fields() {
    let buffer = Arc::new(LogBuffer::default());
    let subscriber = tracing_subscriber::registry().with(BufferLayer(buffer.clone()));
    let mut new = buffer.subscribe();
    tracing::subscriber::with_default(subscriber, || {
      for n in 0..CAPACITY + 5 {
        tracing::info!(n, sub = "local", "scanned");
      }
      tracing::warn!(error = %"boom", "share root unreadable");
    });
    let records = buffer.snapshot();
    assert_eq!(records.len(), CAPACITY);
    assert_eq!(records[0].message, "scanned n=6");
    let last = records.last().unwrap();
    assert_eq!(last.message, "share root unreadable error=boom");
    assert_eq!(last.level, LogLevel::Warning);
    assert_eq!(last.sub_context, None);
    assert_eq!(records[0].sub_context.as_deref(), Some("local"));
    assert!(new.try_recv().is_ok());
  }
}
