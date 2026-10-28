//! The activity log: what other users on the network did with us (browsed our shares, got our
//! search results, queued and received uploads) and changes to our private room memberships,
//! kept as the last few hundred entries in memory and pushed on the `activity` topic as they
//! happen. Nothing is stored; a restart starts an empty log.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use api_types::activity::{ActivityEntry, ActivityKind};
use api_types::live::LiveFrame;
use chrono::Utc;
use tokio::time::Instant;
use tracing::trace;

use crate::live::Live;

/// How many entries are kept; the oldest go first.
pub const KEPT: usize = 500;

/// Upload requests from one user this close together grow one entry rather than adding one each,
/// so a folder queued file by file reads as one line.
pub const QUEUE_GATHER: Duration = Duration::from_secs(10);

/// The activity log. Entries are made by the parts of the daemon that answer other users.
pub struct Activity {
  live: Live,
  ring: Mutex<Ring>,
}

#[derive(Default)]
struct Ring {
  entries: VecDeque<ActivityEntry>,
  /// the id of the last entry made
  last: u64,
  /// the entry each user's latest upload requests went into and when they last grew it
  queues: HashMap<String, (u64, Instant)>,
}

impl Ring {
  /// Adds an entry, the oldest going past the bound.
  fn push(&mut self, username: Option<&str>, kind: ActivityKind) -> ActivityEntry {
    self.last += 1;
    let entry = ActivityEntry {
      id: self.last,
      at: Utc::now(),
      username: username.map(str::to_string),
      kind,
    };
    self.entries.push_back(entry.clone());
    while self.entries.len() > KEPT {
      self.entries.pop_front();
    }
    entry
  }
}

impl Activity {
  /// An empty log whose entries are pushed through `live`.
  pub fn new(live: Live) -> Arc<Self> {
    Arc::new(Self {
      live,
      ring: Mutex::new(Ring::default()),
    })
  }

  fn lock(&self) -> MutexGuard<'_, Ring> {
    // every change to the ring is made whole before the lock is let go
    self.ring.lock().unwrap_or_else(PoisonError::into_inner)
  }

  /// The entries kept, oldest first.
  pub fn snapshot(&self) -> Vec<ActivityEntry> {
    self.lock().entries.iter().cloned().collect()
  }

  /// Notes that `username` did `kind`; `None` for a change to our own private room membership.
  pub fn record(&self, username: Option<&str>, kind: ActivityKind) {
    let entry = self.lock().push(username, kind);
    self.publish(entry);
  }

  /// Notes that `username` queued an upload of `filename`. Requests close together grow the
  /// entry of the ones before.
  pub fn queued(&self, username: &str, filename: &str) {
    let now = Instant::now();
    let entry = {
      let mut ring = self.lock();
      ring
        .queues
        .retain(|_, (_, at)| now.duration_since(*at) < QUEUE_GATHER);
      let open = ring.queues.get(username).map(|(id, _)| *id);
      let grown = open.and_then(|id| ring.entries.iter_mut().find(|e| e.id == id));
      let entry = match grown {
        Some(entry) => {
          if let ActivityKind::Queued {
            files,
            filename: last,
          } = &mut entry.kind
          {
            *files += 1;
            *last = filename.to_string();
          }
          entry.at = Utc::now();
          entry.clone()
        }
        None => ring.push(
          Some(username),
          ActivityKind::Queued {
            files: 1,
            filename: filename.to_string(),
          },
        ),
      };
      ring.queues.insert(username.to_string(), (entry.id, now));
      entry
    };
    self.publish(entry);
  }

  fn publish(&self, entry: ActivityEntry) {
    trace!(id = entry.id, username = entry.username, kind = ?entry.kind, "activity noted");
    self.live.publish(LiveFrame::ActivityEntry(entry));
  }
}

#[cfg(test)]
mod tests {
  use tokio_util::sync::CancellationToken;

  use super::*;

  fn log() -> Arc<Activity> {
    Activity::new(Live::new(CancellationToken::new()))
  }

  #[test]
  fn the_log_keeps_its_newest_entries_numbered_in_order() {
    let activity = log();
    for _ in 0..KEPT + 3 {
      activity.record(Some("alice"), ActivityKind::Browse);
    }
    let kept = activity.snapshot();
    assert_eq!(kept.len(), KEPT);
    assert_eq!(kept.first().unwrap().id, 4);
    assert_eq!(kept.last().unwrap().id, (KEPT + 3) as u64);
  }

  #[tokio::test(start_paused = true)]
  async fn upload_requests_close_together_grow_one_entry_per_user() {
    let activity = log();
    activity.queued("carol", "a\\1.flac");
    activity.queued("dave", "b\\1.flac");
    tokio::time::advance(QUEUE_GATHER / 2).await;
    activity.queued("carol", "a\\2.flac");
    activity.queued("carol", "a\\3.flac");
    tokio::time::advance(QUEUE_GATHER).await;
    activity.queued("carol", "c\\1.flac");
    let kinds: Vec<_> = activity
      .snapshot()
      .into_iter()
      .map(|e| (e.id, e.username.unwrap(), e.kind))
      .collect();
    let queued = |files, filename: &str| ActivityKind::Queued {
      files,
      filename: filename.to_string(),
    };
    assert_eq!(
      kinds,
      [
        (1, "carol".to_string(), queued(3, "a\\3.flac")),
        (2, "dave".to_string(), queued(1, "b\\1.flac")),
        (3, "carol".to_string(), queued(1, "c\\1.flac")),
      ]
    );
  }
}
