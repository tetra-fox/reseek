//! How far each browse of a user has received its reply. The latest progress is kept per user
//! while the browse runs and for five seconds after it ended, so a poll just after still sees
//! how it ended; it is pushed as `users.browse_progress` at most every 250 ms per user, and
//! always once at 100 %. A new browse of the same user takes the place of the last.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use api_types::live::LiveFrame;
use api_types::users::BrowseProgress;
use tokio::time::Instant;

use crate::live::Live;
use crate::sync::lock;

/// How long a browse's progress is kept after it ended.
const KEPT: Duration = Duration::from_secs(5);
/// The least time between two pushes of one user's progress.
const PUSH_EVERY: Duration = Duration::from_millis(250);

struct Entry {
  /// which browse of the user this is
  browse: u64,
  progress: Option<BrowseProgress>,
  pushed: Option<Instant>,
  ended: Option<Instant>,
}

impl Entry {
  /// Whether progress at `percent` is pushed now: the first, the complete one, and any other
  /// 250 ms after the last pushed.
  fn push_due(&mut self, percent: f64) -> bool {
    let due = percent >= 100.0 || self.pushed.is_none_or(|at| at.elapsed() >= PUSH_EVERY);
    if due {
      self.pushed = Some(Instant::now());
    }
    due
  }
}

/// The browses of every user.
pub struct Browses {
  live: Live,
  entries: Mutex<HashMap<String, Entry>>,
  next: Mutex<u64>,
}

impl Browses {
  /// No browses yet; progress is pushed to `live`.
  pub fn new(live: Live) -> Self {
    Self {
      live,
      entries: Mutex::new(HashMap::new()),
      next: Mutex::new(0),
    }
  }

  fn entries(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
    lock(&self.entries)
  }

  /// A browse of `username` starts; returns its number, which its progress and its end name.
  pub fn started(&self, username: &str) -> u64 {
    let browse = {
      let mut next = lock(&self.next);
      *next += 1;
      *next
    };
    let mut entries = self.entries();
    entries.retain(|_, entry| entry.ended.is_none_or(|at| at.elapsed() < KEPT));
    entries.insert(
      username.to_string(),
      Entry {
        browse,
        progress: None,
        pushed: None,
        ended: None,
      },
    );
    browse
  }

  /// More of the reply to `browse` arrived.
  pub fn progressed(&self, browse: u64, progress: &soulseek_client::BrowseProgress) {
    let mut entries = self.entries();
    let Some(entry) = entries
      .get_mut(&progress.username)
      .filter(|entry| entry.browse == browse)
    else {
      return;
    };
    let shown = BrowseProgress {
      username: progress.username.clone(),
      bytes_transferred: progress.bytes_transferred,
      bytes_remaining: progress.bytes_remaining,
      percent_complete: progress.percent_complete,
      size: progress.size,
    };
    entry.progress = Some(shown.clone());
    if entry.push_due(shown.percent_complete) {
      drop(entries);
      self.live.publish(LiveFrame::UsersBrowseProgress(shown));
    }
  }

  /// `browse` of `username` ended, well or not; its progress is kept a while.
  pub fn ended(&self, username: &str, browse: u64) {
    if let Some(entry) = self
      .entries()
      .get_mut(username)
      .filter(|entry| entry.browse == browse)
    {
      entry.ended = Some(Instant::now());
    }
  }

  /// How far the browse of `username` got, while it runs and for a while after.
  pub fn status(&self, username: &str) -> Option<BrowseProgress> {
    let entries = self.entries();
    let entry = entries.get(username)?;
    if entry.ended.is_some_and(|at| at.elapsed() >= KEPT) {
      return None;
    }
    entry.progress.clone()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio_util::sync::CancellationToken;

  fn progress(received: u64, size: u64) -> soulseek_client::BrowseProgress {
    soulseek_client::BrowseProgress {
      username: "alice".into(),
      bytes_transferred: received,
      bytes_remaining: size - received,
      percent_complete: received as f64 / size as f64 * 100.0,
      size,
    }
  }

  #[tokio::test(start_paused = true)]
  async fn progress_is_pushed_every_quarter_second_and_at_the_end() {
    let mut entry = Entry {
      browse: 1,
      progress: None,
      pushed: None,
      ended: None,
    };
    let mut pushed = Vec::new();
    for (wait, percent) in [(0, 0.0), (100, 10.0), (100, 20.0), (100, 30.0), (10, 100.0)] {
      tokio::time::sleep(Duration::from_millis(wait)).await;
      pushed.push(entry.push_due(percent));
    }
    assert_eq!(pushed, [true, false, false, true, true]);
  }

  #[tokio::test(start_paused = true)]
  async fn progress_is_kept_for_a_while_after_the_end_and_a_new_browse_replaces_it() {
    let browses = Browses::new(Live::new(CancellationToken::new()));
    let first = browses.started("alice");
    assert_eq!(browses.status("alice"), None, "nothing arrived yet");
    browses.progressed(first, &progress(50, 100));
    assert_eq!(browses.status("alice").unwrap().bytes_transferred, 50);
    let second = browses.started("alice");
    browses.progressed(first, &progress(100, 100));
    assert_eq!(
      browses.status("alice"),
      None,
      "the old browse's progress is not taken"
    );
    browses.progressed(second, &progress(100, 100));
    browses.ended("alice", second);
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(browses.status("alice").unwrap().percent_complete, 100.0);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(browses.status("alice"), None);
  }
}
