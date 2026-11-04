//! The files of a batch the analysis queue has no room for: kept by name only, in their order,
//! and put in the queue as it drains, so a batch of any size runs while the queue holds at most
//! its limit. The backlog is locked only to take a file from it, so its length is answered while
//! it feeds the queue; clearing it waits for the one file on its way in, so none joins the queue
//! after.

use std::collections::VecDeque;

use api_types::analysis::{AnalysisBatchQueued, AnalysisBatchRefusal};
use tracing::{debug, warn};

use super::Analysis;
use super::jobs::{QUEUE_LIMIT, Queued};
use crate::api::error::ApiError;
use crate::services::FileTarget;
use crate::sync::lock;

/// The most files kept waiting for room in the queue; past it a batch's files are refused.
const BACKLOG_LIMIT: usize = 100_000;

/// The files waiting for room in the queue, first to join it first.
pub(super) type Backlog = VecDeque<FileTarget>;

impl Analysis {
  /// Queues a batch's files in their order, each named with its place in the batch. Once the
  /// queue is full, or while an earlier batch still waits for room, the rest are kept in the
  /// backlog.
  pub(super) async fn batch(&self, files: Vec<(u32, FileTarget)>) -> AnalysisBatchQueued {
    let mut result = AnalysisBatchQueued {
      queued: 0,
      following: 0,
      current: 0,
      refused: Vec::new(),
    };
    let mut kept_back = !self.backlog.lock().await.is_empty();
    for (index, target) in files {
      if !kept_back {
        match self.queue_target(&target).await {
          Ok(Queued::Job(_)) => result.queued += 1,
          Ok(Queued::Current | Queued::Already) => result.current += 1,
          Ok(Queued::Full) => kept_back = true,
          Err(e) => result.refused.push(AnalysisBatchRefusal {
            index,
            error: e.message().to_string(),
          }),
        }
        if !kept_back {
          continue;
        }
      }
      let mut backlog = self.backlog.lock().await;
      if backlog.len() >= BACKLOG_LIMIT {
        result.refused.push(AnalysisBatchRefusal {
          index,
          error: format!("The analysis backlog is full ({BACKLOG_LIMIT} files)"),
        });
      } else {
        backlog.push_back(target);
        result.following += 1;
      }
    }
    if result.following > 0 {
      self.wake.notify_one();
    }
    result
  }

  /// Puts backlog files in the queue while it has room; true when any was queued. The files
  /// that could not be queued are warned about once, with how many there were.
  pub(super) async fn feed(&self) -> bool {
    let mut fed = false;
    // the files not queued, with the first one's reason
    let mut refused: Option<(FileTarget, String)> = None;
    let mut refusals = 0;
    loop {
      let _feeding = self.feeding.lock().await;
      if lock(&self.queue).waiting.len() >= QUEUE_LIMIT {
        break;
      }
      let Some(target) = self.backlog.lock().await.pop_front() else {
        break;
      };
      match self.queue_target(&target).await {
        Ok(Queued::Job(_)) => fed = true,
        Ok(Queued::Current | Queued::Already) => {
          debug!(?target, "file of a batch analyzed or queued already");
        }
        Ok(Queued::Full) => {
          self.backlog.lock().await.push_front(target);
          break;
        }
        // the file can be gone or changed since the batch named it
        Err(e) => {
          debug!(
            ?target,
            error = e.message(),
            "file of a batch not queued for analysis"
          );
          refusals += 1;
          refused.get_or_insert_with(|| (target, e.message().to_string()));
        }
      }
    }
    if let Some((first, error)) = refused {
      warn!(
        files = refusals,
        ?first,
        error,
        "files of a batch not queued for analysis"
      );
    }
    fed
  }

  /// How many files wait in the backlog.
  pub(super) async fn backlog_len(&self) -> usize {
    self.backlog.lock().await.len()
  }

  /// Drops every file of the backlog and every waiting job, returning how many of each. A file
  /// on its way from the backlog into the queue is waited for, so none joins it after.
  pub(super) async fn clear_waiting(&self) -> (usize, usize) {
    let _feeding = self.feeding.lock().await;
    let mut backlog = self.backlog.lock().await;
    let waiting = std::mem::take(&mut lock(&self.queue).waiting).len();
    let dropped = backlog.len();
    backlog.clear();
    (waiting, dropped)
  }

  /// Queues the file `target` names, unless its report is current.
  async fn queue_target(&self, target: &FileTarget) -> Result<Queued, ApiError> {
    let file = self.files.local_file(target).await?;
    self.enqueue(file, false).await
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::time::Duration;

  use api_types::files::FileRoot;

  use super::*;
  use crate::db::Location;
  use crate::testing::{TRUSTED, app};

  #[tokio::test]
  async fn the_backlog_says_its_length_while_a_file_is_on_its_way_in() {
    let app = app(TRUSTED, &[]).await;
    let state = &app.state;
    let analysis = Analysis::open(
      &Location::memory(),
      &app.dir.path().join("own"),
      state.files.clone(),
      state.config.clone(),
      state.live.clone(),
      state.events.clone(),
      state.shutdown.clone(),
    )
    .await
    .unwrap();
    let mut backlog = Backlog::new();
    for name in ["a.flac", "b.flac"] {
      std::fs::write(state.paths.downloads.join(name), b"audio").unwrap();
      backlog.push_back(FileTarget::File {
        root: FileRoot::Downloads,
        path: name.to_string(),
      });
    }
    // the analysis database is held, so the first file waits on it while it is fed
    let (entered, held) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::channel::<()>();
    let db = analysis.db.clone();
    let holder = tokio::spawn(async move {
      db.write(move |_| {
        // fails only when the test stopped waiting, which leaves no one to tell
        let _ = entered.send(());
        // released by the test, or by its end
        let _ = waiting.recv();
        Ok(())
      })
      .await
    });
    held.await.unwrap();
    *analysis.backlog.lock().await = backlog;
    let feeder = tokio::spawn({
      let analysis = Arc::clone(&analysis);
      async move { analysis.feed().await }
    });
    let answered = tokio::time::timeout(Duration::from_secs(10), async {
      while analysis.backlog_len().await > 1 {
        tokio::task::yield_now().await;
      }
    })
    .await;
    drop(release);
    assert!(answered.is_ok(), "the backlog's length waited on the feed");
    holder.await.unwrap().unwrap();
    feeder.await.unwrap();
  }
}
