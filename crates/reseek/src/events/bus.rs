//! The event bus: something that happened is raised once and handed to every subscriber, such
//! as the push to the web UI, the user's scripts and the event log.
//!
//! Raising never fails and never waits: the event's id and time are fixed when it is raised,
//! then each subscriber's own task takes it from a broadcast channel. A subscriber that fails
//! or panics is logged and keeps receiving; a slow one misses events rather than holding up
//! the others. Subscribers are named, and subscribing a name again replaces the earlier one.

use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, PoisonError};

use api_types::events::{Event, EventRecord, EventType};
use chrono::{DateTime, Utc};
use futures_util::FutureExt;
use soulseek_client::LogLimiter;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info_span, warn};

/// How many events a slow subscriber may fall behind by before it misses some.
const BACKLOG: usize = 1024;

/// The event bus. Cloning gives another handle to the same bus.
#[derive(Clone)]
pub struct EventBus {
  inner: Arc<Inner>,
}

/// A subscriber's name and the one type it takes, `None` for every type.
type Subscriber = (String, Option<EventType>);

struct Inner {
  sender: broadcast::Sender<Arc<EventRecord>>,
  /// each subscriber's task
  subscribers: Mutex<HashMap<Subscriber, JoinHandle<()>>>,
  cancel: CancellationToken,
}

impl Drop for Inner {
  fn drop(&mut self) {
    let subscribers = self
      .subscribers
      .get_mut()
      .unwrap_or_else(PoisonError::into_inner);
    for (_, task) in subscribers.drain() {
      task.abort();
    }
  }
}

impl EventBus {
  /// A bus with no subscribers, whose subscriber tasks stop when `cancel` is cancelled.
  pub fn new(cancel: CancellationToken) -> Self {
    Self {
      inner: Arc::new(Inner {
        sender: broadcast::Sender::new(BACKLOG),
        subscribers: Mutex::new(HashMap::new()),
        cancel,
      }),
    }
  }

  /// Raises `event` as having happened now.
  pub fn raise(&self, event: Event) -> Arc<EventRecord> {
    self.raise_at(event, Utc::now())
  }

  /// Raises `event` as having happened at `timestamp`, such as when a transfer ended.
  pub fn raise_at(&self, event: Event, timestamp: DateTime<Utc>) -> Arc<EventRecord> {
    let record = Arc::new(EventRecord {
      id: uuid::Uuid::new_v4(),
      timestamp,
      event,
    });
    debug!(id = %record.id, kind = record.event.event_type().name(), "event raised");
    // an error only means nothing is subscribed
    drop(self.inner.sender.send(record.clone()));
    record
  }

  /// Hands every event, or only those of type `only`, to `handler` on a task of its own. A
  /// subscriber with the same name and type replaces the earlier one.
  pub fn subscribe<F, Fut>(&self, name: &str, only: Option<EventType>, handler: F)
  where
    F: Fn(Arc<EventRecord>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
  {
    let receiver = self.inner.sender.subscribe();
    let span = info_span!("subscriber", name);
    let task = tokio::spawn(
      deliver(
        name.to_string(),
        only,
        receiver,
        handler,
        self.inner.cancel.clone(),
      )
      .instrument(span),
    );
    let replaced = self
      .inner
      .subscribers
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .insert((name.to_string(), only), task);
    if let Some(earlier) = replaced {
      earlier.abort();
      debug!(
        name,
        "attempted to create a redundant subscription. The existing subscription was overwritten"
      );
    }
  }

  /// Removes the subscriber `name` for `only`; nothing happens when there is none.
  pub fn unsubscribe(&self, name: &str, only: Option<EventType>) {
    let removed = self
      .inner
      .subscribers
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .remove(&(name.to_string(), only));
    if let Some(task) = removed {
      task.abort();
    }
  }
}

/// A subscriber's task: takes each event it wants and runs the handler on it in turn.
async fn deliver<F, Fut>(
  name: String,
  only: Option<EventType>,
  mut receiver: broadcast::Receiver<Arc<EventRecord>>,
  handler: F,
  cancel: CancellationToken,
) where
  F: Fn(Arc<EventRecord>) -> Fut,
  Fut: Future<Output = Result<(), String>>,
{
  // a subscriber that keeps falling behind would log on every event
  let behind = LogLimiter::new();
  loop {
    let record = tokio::select! {
      () = cancel.cancelled() => return,
      record = receiver.recv() => record,
    };
    let record = match record {
      Ok(record) => record,
      Err(RecvError::Lagged(missed)) => {
        if let Some(suppressed) = behind.allow() {
          warn!(
            subscriber = name,
            missed, suppressed, "event subscriber fell behind and missed events"
          );
        }
        continue;
      }
      Err(RecvError::Closed) => return,
    };
    let kind = record.event.event_type();
    if only.is_some_and(|wanted| wanted != kind) {
      continue;
    }
    match AssertUnwindSafe(handler(record)).catch_unwind().await {
      Ok(Ok(())) => {}
      Ok(Err(e)) => error!(
        subscriber = name,
        kind = kind.name(),
        error = e,
        "Subscriber encountered an error"
      ),
      Err(panic) => {
        error!(
          subscriber = name,
          kind = kind.name(),
          panic = crate::logging::panic_message(panic.as_ref()),
          "Subscriber panicked"
        );
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use api_types::events::NoFields;
  use std::time::Duration;
  use tokio::sync::mpsc;

  fn noop() -> Event {
    Event::Noop(NoFields { version: 0 })
  }

  #[tokio::test]
  async fn raising_fixes_id_and_time_and_a_failing_subscriber_harms_no_one() {
    let bus = EventBus::new(CancellationToken::new());
    assert!(
      !bus.raise(noop()).id.is_nil(),
      "raising with no subscriber is fine"
    );
    let (seen, mut received) = mpsc::unbounded_channel();
    let seen_typed = seen.clone();
    bus.subscribe("failing", None, |_| async {
      Err::<(), _>("broken".to_string())
    });
    bus.subscribe("panicking", None, |_| async { panic!("boom") });
    bus.subscribe("all", None, move |record: Arc<EventRecord>| {
      let seen = seen.clone();
      async move {
        seen.send(("all", record.id)).unwrap();
        Ok(())
      }
    });
    bus.subscribe(
      "typed",
      Some(EventType::LoginFailed),
      move |record: Arc<EventRecord>| {
        let seen = seen_typed.clone();
        async move {
          seen.send(("typed", record.id)).unwrap();
          Ok(())
        }
      },
    );
    let at = "2025-03-01T12:00:00Z".parse().unwrap();
    let first = bus.raise_at(noop(), at);
    assert_eq!(first.timestamp, at);
    let second = bus.raise(noop());
    let mut got = Vec::new();
    for _ in 0..2 {
      got.push(
        tokio::time::timeout(Duration::from_secs(5), received.recv())
          .await
          .unwrap()
          .unwrap(),
      );
    }
    assert_eq!(got, [("all", first.id), ("all", second.id)]);
    assert!(
      received.try_recv().is_err(),
      "the typed one takes only its type"
    );
  }

  #[tokio::test]
  async fn subscribing_a_name_again_replaces_it() {
    let bus = EventBus::new(CancellationToken::new());
    let (seen, mut received) = mpsc::unbounded_channel();
    for label in ["first", "second"] {
      let seen = seen.clone();
      bus.subscribe("scripts", None, move |_| {
        let seen = seen.clone();
        async move {
          seen.send(label).unwrap();
          Ok(())
        }
      });
    }
    bus.raise(noop());
    let got = tokio::time::timeout(Duration::from_secs(5), received.recv())
      .await
      .unwrap();
    assert_eq!(got, Some("second"));
    bus.unsubscribe("scripts", None);
    bus.unsubscribe("never subscribed", None);
  }
}
