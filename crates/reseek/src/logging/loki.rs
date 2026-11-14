//! The Grafana Loki sink: every line the filter lets through is queued, without waiting, for a
//! task that pushes the lines in batches to Loki's push API, each labeled with the process's
//! instance name, invocation id and process id and its level. A full queue drops a line and
//! counts it, and the count goes to Loki with the next batch; a batch Loki does not take is
//! tried again twice, then dropped.

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use soulseek_client::LogLimiter;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio_util::sync::CancellationToken;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::{Instrument, Level, Metadata, Subscriber, warn};
use tracing_subscriber::filter::dynamic_filter_fn;
use tracing_subscriber::fmt::{Layer as FmtLayer, MakeWriter};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use super::PROCESS_SPAN;
use reseek_shares::log_format::{Clock, Line};

/// How many lines may wait to be pushed before new ones are dropped.
pub const CAPACITY: usize = 8192;

/// The most lines one push carries.
const BATCH: usize = 1000;

/// How long lines are gathered into a batch after the first.
const GATHERING: Duration = Duration::from_secs(1);

/// How often a batch is pushed before it is dropped.
const TRIES: u32 = 3;
/// The wait before the first retry, doubled for each later one.
const RETRY: Duration = Duration::from_secs(1);

/// The span the pushing task runs in. Its own lines stay out of Loki, where a failing push would
/// only add more to push.
const WORKER_SPAN: &str = "loki";

/// How long the last lines may take to be pushed when the daemon stops.
const FINISHING: Duration = Duration::from_secs(5);

static NOT_PUSHED: LogLimiter = LogLimiter::new();
static DROPPED: LogLimiter = LogLimiter::new();

/// A line waiting to be pushed.
#[derive(Debug)]
struct Entry {
  at: SystemTime,
  level: Level,
  line: String,
}

/// Where the lines are queued for the pushing task, and how many the full queue dropped.
#[derive(Clone)]
pub struct Queue {
  sender: mpsc::Sender<Entry>,
  dropped: Arc<AtomicU64>,
}

/// The labels every line is pushed with: the fields of the process span, once it exists.
#[derive(Clone, Default)]
pub struct Labels(Arc<OnceLock<BTreeMap<String, String>>>);

/// The pushing end: takes the queued lines and pushes them to one Loki instance.
pub struct Sink {
  push_url: String,
  receiver: mpsc::Receiver<Entry>,
  dropped: Arc<AtomicU64>,
  labels: Labels,
}

/// A queue of `capacity` lines and the sink that pushes them to the Loki instance at `url`, with
/// the process span's fields that `labels` takes as their labels.
pub fn channel(url: &str, labels: Labels, capacity: usize) -> (Queue, Sink) {
  let (sender, receiver) = mpsc::channel(capacity);
  let dropped = Arc::new(AtomicU64::new(0));
  let queue = Queue {
    sender,
    dropped: dropped.clone(),
  };
  let sink = Sink {
    push_url: format!("{}/loki/api/v1/push", url.trim_end_matches('/')),
    receiver,
    dropped,
    labels,
  };
  (queue, sink)
}

/// The layer that queues every line, in the file's format without its time, for the sink; the
/// pushing task's own lines are left out.
pub fn lines<S>(queue: Queue) -> impl Layer<S> + Send + Sync + 'static
where
  S: Subscriber + for<'a> LookupSpan<'a>,
{
  FmtLayer::default()
    .event_format(Line {
      clock: Clock::Off,
      color: false,
    })
    .with_ansi(false)
    .with_writer(queue)
    .with_filter(dynamic_filter_fn(|metadata, cx: &Context<'_, S>| {
      metadata.is_span()
        || !cx
          .lookup_current()
          .is_some_and(|span| span.scope().any(|span| span.name() == WORKER_SPAN))
    }))
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Labels {
  fn on_new_span(&self, attributes: &Attributes<'_>, _id: &Id, _cx: Context<'_, S>) {
    if attributes.metadata().name() != PROCESS_SPAN {
      return;
    }
    let mut fields = Fields(BTreeMap::from([("app".to_string(), "reseek".to_string())]));
    attributes.record(&mut fields);
    // the process has one such span
    drop(self.0.set(fields.0));
  }
}

/// A span's fields as text.
struct Fields(BTreeMap<String, String>);

impl Visit for Fields {
  fn record_str(&mut self, field: &Field, value: &str) {
    self.0.insert(field.name().to_string(), value.to_string());
  }

  fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
    self
      .0
      .insert(field.name().to_string(), format!("{value:?}"));
  }
}

/// One line as the format writes it, queued when it is done.
pub struct Writer {
  queue: Queue,
  /// when the event was logged, before its line was formatted
  at: SystemTime,
  level: Level,
  line: Vec<u8>,
}

impl<'a> MakeWriter<'a> for Queue {
  type Writer = Writer;

  fn make_writer(&'a self) -> Writer {
    self.writer(Level::INFO)
  }

  fn make_writer_for(&'a self, metadata: &Metadata<'_>) -> Writer {
    self.writer(*metadata.level())
  }
}

impl Queue {
  fn writer(&self, level: Level) -> Writer {
    Writer {
      queue: self.clone(),
      at: SystemTime::now(),
      level,
      line: Vec::new(),
    }
  }
}

impl Write for Writer {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    self.line.extend_from_slice(bytes);
    Ok(bytes.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

impl Drop for Writer {
  fn drop(&mut self) {
    let line = String::from_utf8_lossy(&self.line).trim_end().to_string();
    let entry = Entry {
      at: self.at,
      level: self.level,
      line,
    };
    match self.queue.sender.try_send(entry) {
      Ok(()) => {}
      Err(TrySendError::Full(_)) => {
        self.queue.dropped.fetch_add(1, Ordering::Relaxed);
      }
      // the sink stopped with the daemon; nothing is pushed any more
      Err(TrySendError::Closed(_)) => {}
    }
  }
}

impl Sink {
  /// Pushes the queued lines in batches until `stop`, then what was queued by then.
  pub async fn run(mut self, client: reqwest::Client, stop: CancellationToken) {
    loop {
      let first = tokio::select! {
        entry = self.receiver.recv() => entry,
        () = stop.cancelled() => break,
      };
      let Some(first) = first else { return };
      let mut batch = vec![first];
      let gathered = tokio::time::sleep(GATHERING);
      tokio::pin!(gathered);
      while batch.len() < BATCH {
        tokio::select! {
          entry = self.receiver.recv() => match entry {
            Some(entry) => batch.push(entry),
            None => break,
          },
          () = &mut gathered => break,
          () = stop.cancelled() => break,
        }
      }
      self.push(&client, batch).await;
    }
    self.receiver.close();
    let mut last = Vec::new();
    while let Ok(entry) = self.receiver.try_recv() {
      last.push(entry);
    }
    for batch in last.chunks(BATCH) {
      if let Err(e) = self.push_once(&client, batch).await {
        warn!(
          error = e,
          lines = batch.len(),
          "the last log lines not pushed to Loki"
        );
      }
    }
  }

  /// Pushes `batch`, with a line saying how many were dropped since the last push, trying
  /// again when Loki does not take it.
  async fn push(&self, client: &reqwest::Client, mut batch: Vec<Entry>) {
    let dropped = self.dropped.swap(0, Ordering::Relaxed);
    if dropped > 0 {
      if let Some(suppressed) = DROPPED.allow() {
        warn!(
          dropped,
          suppressed, "log lines dropped before they reached Loki"
        );
      }
      batch.push(Entry {
        at: SystemTime::now(),
        level: Level::WARN,
        line: format!(
          "[WRN] {dropped} log lines dropped before they reached Loki; its queue was full"
        ),
      });
    }
    let mut wait = RETRY;
    for attempt in 1..=TRIES {
      match self.push_once(client, &batch).await {
        Ok(()) => return,
        Err(e) if attempt == TRIES => {
          if let Some(suppressed) = NOT_PUSHED.allow() {
            warn!(
              error = e,
              lines = batch.len(),
              suppressed,
              "log lines not pushed to Loki; dropped"
            );
          }
        }
        Err(e) => {
          if let Some(suppressed) = NOT_PUSHED.allow() {
            warn!(
              error = e,
              attempt, suppressed, "log lines not pushed to Loki; trying again"
            );
          }
          tokio::time::sleep(wait).await;
          wait *= 2;
        }
      }
    }
  }

  /// One push of `batch`.
  async fn push_once(&self, client: &reqwest::Client, batch: &[Entry]) -> Result<(), String> {
    let body = body(self.labels.0.get(), batch);
    let response = client
      .post(&self.push_url)
      .header(reqwest::header::CONTENT_TYPE, "application/json")
      .body(body.to_string())
      .send()
      .await
      .map_err(|e| crate::http::outbound::error_chain(&e))?;
    let status = response.status();
    if status.is_success() {
      return Ok(());
    }
    // a body that cannot be read leaves the status to say what went wrong
    let text = response.text().await.unwrap_or_default();
    Err(format!("Loki answered {status}: {}", text.trim()))
  }
}

/// The push body: one stream per level, each line with its time in nanoseconds.
fn body(labels: Option<&BTreeMap<String, String>>, batch: &[Entry]) -> serde_json::Value {
  let mut streams: BTreeMap<&str, Vec<[String; 2]>> = BTreeMap::new();
  for entry in batch {
    let nanos = entry
      .at
      .duration_since(UNIX_EPOCH)
      .map_or(0, |since| since.as_nanos());
    streams
      .entry(level_label(entry.level))
      .or_default()
      .push([nanos.to_string(), entry.line.clone()]);
  }
  let streams: Vec<serde_json::Value> = streams
    .into_iter()
    .map(|(level, values)| {
      let mut stream = labels
        .cloned()
        .unwrap_or_else(|| BTreeMap::from([("app".to_string(), "reseek".to_string())]));
      stream.insert("level".to_string(), level.to_string());
      serde_json::json!({ "stream": stream, "values": values })
    })
    .collect();
  serde_json::json!({ "streams": streams })
}

fn level_label(level: Level) -> &'static str {
  match level {
    Level::TRACE => "trace",
    Level::DEBUG => "debug",
    Level::INFO => "info",
    Level::WARN => "warn",
    Level::ERROR => "error",
  }
}

/// The pushing task once it runs: stopping it pushes what is queued first.
pub struct Running {
  stop: CancellationToken,
  task: tokio::task::JoinHandle<()>,
}

impl Sink {
  /// Starts pushing on a task of its own, with `client`.
  pub fn start(self, client: reqwest::Client) -> Running {
    let stop = CancellationToken::new();
    let task = tokio::spawn(
      self
        .run(client, stop.clone())
        .instrument(tracing::info_span!(WORKER_SPAN)),
    );
    Running { stop, task }
  }
}

impl Running {
  /// Pushes what is queued, giving it a few seconds, and stops.
  pub async fn finish(self) {
    self.stop.cancel();
    match tokio::time::timeout(FINISHING, self.task).await {
      Ok(Ok(())) => {}
      Ok(Err(e)) => warn!(error = %e, "the Loki sink ended badly"),
      Err(_) => warn!("the last log lines were not pushed to Loki in time"),
    }
  }
}

#[cfg(test)]
mod tests;
