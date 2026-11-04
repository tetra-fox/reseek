//! Noticing that the configuration files changed: file notifications or polling, and waiting
//! for a burst of them to end.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::files::Entry;
use crate::lock;

/// How long the files must be quiet after a change before they are read: a save is often
/// several events.
pub(super) const DEBOUNCE: Duration = Duration::from_millis(250);

/// Waits until `events` has been quiet for `period`. False when canceled first.
pub(super) async fn quiet(events: &Notify, period: Duration, cancel: &CancellationToken) -> bool {
  loop {
    tokio::select! {
      () = cancel.cancelled() => return false,
      () = events.notified() => {}
      () = tokio::time::sleep(period) => return true,
    }
  }
}

/// The files' directories under watch.
pub(super) struct FileEvents {
  watcher: Box<dyn notify::Watcher + Send>,
  /// every directory a file or a pattern's matches live in
  dirs: Vec<PathBuf>,
  watched: BTreeSet<PathBuf>,
  /// directories an event was about itself, such as one removed, renamed or made again, whose
  /// watch may have ended with it
  touched: Arc<Mutex<BTreeSet<PathBuf>>>,
}

impl FileEvents {
  /// Watches the directories of `entries`, by notifications or, with `poll`, by looking at that
  /// interval, and wakes `events` for each change to one of their files.
  pub(super) fn new(
    poll: Option<Duration>,
    entries: Vec<Entry>,
    events: Arc<Notify>,
  ) -> notify::Result<Self> {
    let mut dirs: Vec<PathBuf> = entries
      .iter()
      .filter_map(|entry| match entry.plain_path() {
        Some(path) => path.parent().map(Path::to_path_buf),
        None => entry.pattern_dir(),
      })
      .collect();
    dirs.sort();
    dirs.dedup();
    let relevant_dirs = dirs.clone();
    let touched = Arc::new(Mutex::new(BTreeSet::new()));
    let touched_dirs = touched.clone();
    let handler = move |result: notify::Result<notify::Event>| match result {
      Ok(event) => {
        if matches!(event.kind, notify::EventKind::Access(_)) {
          return;
        }
        let mut relevant = false;
        for path in &event.paths {
          if relevant_dirs.contains(path) {
            lock(&touched_dirs).insert(path.clone());
            relevant = true;
          } else if named(&entries, path) {
            relevant = true;
          }
        }
        if relevant {
          events.notify_one();
        }
      }
      Err(e) => warn!(error = %e, "configuration file watch failed"),
    };
    let watcher: Box<dyn notify::Watcher + Send> = match poll {
      // the poller compares modification times in whole seconds, so a save in the same second
      // as its last look would be missed for good; comparing contents catches it
      Some(interval) => Box::new(notify::PollWatcher::new(
        handler,
        notify::Config::default()
          .with_poll_interval(interval)
          .with_compare_contents(true),
      )?),
      None => Box::new(notify::recommended_watcher(handler)?),
    };
    let mut files = Self {
      watcher,
      dirs,
      watched: BTreeSet::new(),
      touched,
    };
    files.watch_new_dirs();
    Ok(files)
  }

  /// Watches each directory the files live in that exists and is not watched yet, or the
  /// parent of one that does not exist, to see it made. A directory an event was about is
  /// watched anew, since removing it ends its watch even when it is made again, and one no
  /// longer wanted is let go.
  pub(super) fn watch_new_dirs(&mut self) {
    let touched = std::mem::take(&mut *lock(&self.touched));
    let wanted: BTreeSet<PathBuf> = self
      .dirs
      .iter()
      .map(|dir| match (dir.exists(), dir.parent()) {
        (false, Some(parent)) => parent.to_path_buf(),
        _ => dir.clone(),
      })
      .collect();
    let stale: Vec<PathBuf> = self
      .watched
      .iter()
      .filter(|dir| touched.contains(*dir) || !wanted.contains(*dir))
      .cloned()
      .collect();
    for dir in stale {
      self.watched.remove(&dir);
      // a removed directory's watch ended with it, which the system reports as not watched
      if let Err(e) = self.watcher.unwatch(&dir) {
        debug!(dir = %dir.display(), error = %e, "configuration directory watch already gone");
      }
    }
    for dir in wanted {
      if self.watched.contains(&dir) {
        continue;
      }
      match self
        .watcher
        .watch(&dir, notify::RecursiveMode::NonRecursive)
      {
        Ok(()) => {
          debug!(dir = %dir.display(), "watching configuration directory");
          self.watched.insert(dir);
        }
        Err(e) => warn!(dir = %dir.display(), error = %e, "cannot watch configuration directory"),
      }
    }
  }
}

/// Whether `path` is a file one of `entries` names.
fn named(entries: &[Entry], path: &Path) -> bool {
  let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
    return false;
  };
  entries.iter().any(|entry| entry.names(dir, name))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{next_change, values, watched};
  use crate::{Source, Watch};
  use serde_json::json;
  use std::fs;

  #[tokio::test(start_paused = true)]
  async fn quiet_waits_for_events_to_stop() {
    let events = Arc::new(Notify::new());
    let cancel = CancellationToken::new();
    let started = tokio::time::Instant::now();
    let poker = {
      let events = events.clone();
      tokio::spawn(async move {
        for _ in 0..3 {
          events.notify_one();
          tokio::time::sleep(Duration::from_millis(100)).await;
        }
      })
    };
    tokio::task::yield_now().await;
    assert!(quiet(&events, DEBOUNCE, &cancel).await);
    poker.await.unwrap();
    let waited = started.elapsed();
    assert!(
      waited >= Duration::from_millis(200) + DEBOUNCE && waited < Duration::from_millis(600),
      "{waited:?}"
    );
    cancel.cancel();
    assert!(!quiet(&events, DEBOUNCE, &cancel).await);
  }

  #[tokio::test]
  async fn a_burst_of_saves_is_one_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[server]\nport = 2000\n").unwrap();
    let watcher = watched(dir.path());
    let mut changes = watcher.changes();
    let cancel = CancellationToken::new();
    watcher.start(Watch::Events, cancel.clone());

    for port in 2001..=2005 {
      fs::write(&path, format!("[server]\nport = {port}\n")).unwrap();
      tokio::time::sleep(Duration::from_millis(40)).await;
    }
    let change = next_change(&mut changes).await;
    assert_eq!(change.current.values.server.port, 2005);
    assert_eq!(change.previous.values.server.port, 2000);
    let more = tokio::time::timeout(DEBOUNCE * 3, changes.recv()).await;
    assert!(more.is_err(), "one reload for the burst: {more:?}");

    // a drop-in made after the start, in a directory made after the start
    fs::create_dir(dir.path().join("config.d")).unwrap();
    tokio::time::sleep(DEBOUNCE * 2).await;
    fs::write(
      dir.path().join("config.d/10-a.toml"),
      "[server]\naddress = \"drop-in\"\n",
    )
    .unwrap();
    let change = next_change(&mut changes).await;
    assert_eq!(change.changed, ["server.address"]);
    assert_eq!(
      change.current.sources["server.address"],
      Source::File {
        file: "config.d/10-a.toml".into()
      }
    );
    cancel.cancel();
  }

  #[tokio::test]
  async fn a_drop_in_directory_made_again_is_watched_again() {
    let dir = tempfile::tempdir().unwrap();
    let drop_ins = dir.path().join("config.d");
    fs::create_dir(&drop_ins).unwrap();
    let watcher = watched(dir.path());
    let mut changes = watcher.changes();
    let cancel = CancellationToken::new();
    watcher.start(Watch::Events, cancel.clone());

    // removed and made again in one burst, which ends the old directory's watch
    fs::remove_dir_all(&drop_ins).unwrap();
    fs::create_dir(&drop_ins).unwrap();
    tokio::time::sleep(DEBOUNCE * 2).await;
    fs::write(drop_ins.join("10-a.toml"), "[server]\nport = 2001\n").unwrap();
    assert_eq!(
      next_change(&mut changes).await.current.values.server.port,
      2001
    );
    cancel.cancel();
  }

  #[tokio::test]
  async fn polling_sees_changes_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[server]\nport = 2000\n").unwrap();
    let watcher = watched(dir.path());
    let mut changes = watcher.changes();
    let cancel = CancellationToken::new();
    watcher.start(Watch::Poll(Duration::from_millis(50)), cancel.clone());
    fs::write(
      &path,
      "[server]\nport = 2001\n# longer, so the size changes too\n",
    )
    .unwrap();
    assert_eq!(
      next_change(&mut changes).await.current.values.server.port,
      2001
    );
    cancel.cancel();
  }

  #[tokio::test]
  async fn our_own_write_is_not_loaded_twice() {
    let dir = tempfile::tempdir().unwrap();
    let watcher = watched(dir.path());
    let mut changes = watcher.changes();
    let cancel = CancellationToken::new();
    watcher.start(Watch::Events, cancel.clone());
    watcher
      .patch_file(
        "config.toml",
        None,
        values(&[("server.port", json!(3000))]),
        vec![],
      )
      .await
      .unwrap();
    next_change(&mut changes).await;
    let again = tokio::time::timeout(DEBOUNCE * 4, changes.recv()).await;
    assert!(again.is_err(), "{again:?}");
    cancel.cancel();
  }
}
