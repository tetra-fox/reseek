//! One open stream: its queue of published frames, and the task that writes the stream with
//! keep-alive comments, at most one application state every 100 ms, a fresh check of its
//! credential every 30 s, and a goodbye before it closes.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use api_types::live::{Bye, ByeReason, LiveFrame};
use axum::body::{Body, Bytes};
use tokio::sync::{Notify, mpsc};
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, trace, warn};

use super::{Published, Registered, Topic, render};

/// How many frames a stream may fall behind by before it is closed as lagging.
const QUEUE: usize = 1024;

/// How long a stream stays silent before a keep-alive comment.
const PING: Duration = Duration::from_secs(15);

/// How often a stream's credential is checked again.
const RECHECK: Duration = Duration::from_secs(30);

/// The least time between two application states on one stream.
const STATE_GAP: Duration = Duration::from_millis(100);

/// How many written pieces wait for the connection to take them.
const OUTPUT: usize = 16;

/// Asks whether the stream's credential is still good.
pub type Revalidate = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// The frames published for one stream and not yet written.
pub struct Queue {
  topics: Vec<Topic>,
  inner: Mutex<Pending>,
  ready: Notify,
}

#[derive(Default)]
struct Pending {
  frames: VecDeque<Arc<Published>>,
  /// how many of `frames` are application states
  states: usize,
  /// a frame did not fit, so the stream must close
  overflowed: bool,
}

/// What the stream writes next.
enum Next {
  Write(Arc<Published>),
  /// an application state is due no sooner than this
  WaitUntil(Instant),
  Nothing,
  Overflowed,
}

impl Queue {
  pub(super) fn new(topics: Vec<Topic>) -> Self {
    Self {
      topics,
      inner: Mutex::new(Pending::default()),
      ready: Notify::new(),
    }
  }

  /// Whether the stream follows one of `topics`.
  pub(super) fn follows(&self, topics: &[Topic]) -> bool {
    topics.iter().any(|topic| self.topics.contains(topic))
  }

  /// Queues `frame`. A full queue first drops its oldest application state that a later one
  /// replaces; when there is none the frame does not fit, the stream is marked to close, and
  /// this returns false.
  pub(super) fn push(&self, frame: Arc<Published>) -> bool {
    let mut pending = self.lock();
    if pending.overflowed {
      return false;
    }
    if pending.frames.len() >= QUEUE {
      let replaced = pending.states + usize::from(frame.state) >= 2;
      match pending.frames.iter().position(|f| f.state) {
        Some(index) if replaced => {
          pending.frames.remove(index);
          pending.states -= 1;
        }
        _ => {
          pending.overflowed = true;
          drop(pending);
          self.ready.notify_one();
          return false;
        }
      }
    }
    pending.states += usize::from(frame.state);
    pending.frames.push_back(frame);
    drop(pending);
    self.ready.notify_one();
    true
  }

  pub(super) fn clear(&self) {
    *self.lock() = Pending::default();
  }

  /// The next frame to write, skipping application states a later one replaces and holding
  /// one back while the last was written less than [`STATE_GAP`] ago.
  fn next(&self, last_state: Option<Instant>) -> Next {
    let mut pending = self.lock();
    if pending.overflowed {
      return Next::Overflowed;
    }
    loop {
      let Some(front) = pending.frames.front() else {
        return Next::Nothing;
      };
      if front.state {
        if pending.states > 1 {
          pending.frames.pop_front();
          pending.states -= 1;
          continue;
        }
        if let Some(due) = last_state.map(|at| at + STATE_GAP)
          && Instant::now() < due
        {
          return Next::WaitUntil(due);
        }
      }
      let frame = pending.frames.pop_front().expect("the front exists");
      pending.states -= usize::from(frame.state);
      return Next::Write(frame);
    }
  }

  fn lock(&self) -> MutexGuard<'_, Pending> {
    // every change is made whole before the lock is let go
    self.inner.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

/// The body of an open stream: `retry`, then `first` (the hello and any snapshots), then what
/// is published for it, until the client leaves, the credential stops being good, the stream
/// falls too far behind, or the daemon shuts down.
pub fn body(registered: Registered, first: Vec<Bytes>, revalidate: Revalidate) -> Body {
  let cancel = registered.hub.cancel.clone();
  let (output, receiver) = mpsc::channel(OUTPUT);
  tokio::spawn(write(registered, first, revalidate, cancel, output).in_current_span());
  Body::from_stream(futures_util::stream::unfold(
    receiver,
    |mut receiver| async move {
      receiver
        .recv()
        .await
        .map(|bytes| (Ok::<_, Infallible>(bytes), receiver))
    },
  ))
}

/// The stream's task.
async fn write(
  registered: Registered,
  first: Vec<Bytes>,
  revalidate: Revalidate,
  cancel: CancellationToken,
  output: mpsc::Sender<Bytes>,
) {
  let queue = registered.queue.clone();
  let mut last_id = registered.at;
  for bytes in std::iter::once(Bytes::from_static(b"retry: 1000\n\n")).chain(first) {
    if output.send(bytes).await.is_err() {
      return;
    }
  }
  let mut last_write = Instant::now();
  let mut recheck = Instant::now() + RECHECK;
  let mut last_state: Option<Instant> = None;
  let reason = loop {
    let mut held = None;
    let mut lagged = false;
    loop {
      match queue.next(last_state) {
        Next::Write(frame) => {
          if frame.state {
            last_state = Some(Instant::now());
          }
          if !frame.topics.contains(&Topic::Logs) {
            trace!(seq = frame.seq, "frame sent");
          }
          if output.send(frame.block.clone()).await.is_err() {
            debug!("event stream closed by the client");
            return;
          }
          last_id = frame.seq;
          last_write = Instant::now();
        }
        Next::WaitUntil(due) => {
          held = Some(due);
          break;
        }
        Next::Nothing => break,
        Next::Overflowed => {
          lagged = true;
          break;
        }
      }
    }
    if lagged {
      warn!("event stream fell too far behind; closing it");
      break ByeReason::Lagged;
    }
    let far = Instant::now() + RECHECK;
    tokio::select! {
      () = cancel.cancelled() => break ByeReason::Shutdown,
      () = output.closed() => {
        debug!("event stream closed by the client");
        return;
      }
      () = queue.ready.notified() => {}
      () = sleep_until(held.unwrap_or(far)), if held.is_some() => {}
      () = sleep_until(last_write + PING) => {
        if output.send(Bytes::from_static(b": ping\n\n")).await.is_err() {
          return;
        }
        last_write = Instant::now();
      }
      () = sleep_until(recheck) => {
        if !revalidate().await {
          debug!("event stream credential no longer valid");
          break ByeReason::Unauthorized;
        }
        recheck = Instant::now() + RECHECK;
      }
    }
  };
  match render(last_id, &LiveFrame::Bye(Bye { reason })) {
    // the client may already be gone, which ends the stream all the same
    Ok(bytes) => drop(output.send(bytes).await),
    Err(e) => error!(error = %e, "goodbye frame not written"),
  }
  debug!(?reason, "event stream closed");
}

#[cfg(test)]
mod tests {
  use super::*;

  fn published(seq: u64, state: bool) -> Arc<Published> {
    Arc::new(Published {
      seq,
      at: Instant::now(),
      topics: vec![Topic::Application],
      state,
      block: Bytes::new(),
    })
  }

  fn written(next: Next) -> u64 {
    match next {
      Next::Write(frame) => frame.seq,
      _ => panic!("nothing to write"),
    }
  }

  #[test]
  fn a_later_application_state_replaces_an_earlier_one_and_states_keep_their_gap() {
    let queue = Queue::new(vec![Topic::Application]);
    for (seq, state) in [(1, true), (2, false), (3, true), (4, false)] {
      assert!(queue.push(published(seq, state)));
    }
    assert_eq!(
      written(queue.next(None)),
      2,
      "state 1 is replaced by state 3"
    );
    let just_now = Some(Instant::now());
    assert!(
      matches!(queue.next(just_now), Next::WaitUntil(_)),
      "too soon after the last"
    );
    let long_ago = Some(Instant::now() - STATE_GAP);
    assert_eq!(written(queue.next(long_ago)), 3);
    assert_eq!(written(queue.next(long_ago)), 4);
    assert!(matches!(queue.next(None), Next::Nothing));
  }

  #[test]
  fn a_full_queue_drops_replaced_states_first_then_overflows() {
    let queue = Queue::new(vec![Topic::Application]);
    assert!(queue.push(published(0, true)));
    for seq in 1..QUEUE as u64 {
      assert!(queue.push(published(seq, false)));
    }
    assert!(
      queue.push(published(5000, true)),
      "the old state makes room"
    );
    assert!(!queue.push(published(5001, false)), "nothing left to drop");
    assert!(matches!(queue.next(None), Next::Overflowed));
  }
}
