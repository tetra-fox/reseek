//! The files of a batch the analysis queue has no room for: kept by name only, in their order,
//! and put in the queue as it drains, so a batch of any size runs while the queue holds at most
//! its limit. The backlog stays locked while it feeds the queue, so clearing it waits for a file
//! on its way in and none joins the queue after.

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
    let mut backlog = self.backlog.lock().await;
    let mut fed = false;
    // the files not queued, with the first one's reason
    let mut refused: Option<(FileTarget, String)> = None;
    let mut refusals = 0;
    while lock(&self.queue).waiting.len() < QUEUE_LIMIT {
      let Some(target) = backlog.pop_front() else {
        break;
      };
      match self.queue_target(&target).await {
        Ok(Queued::Job(_)) => fed = true,
        Ok(Queued::Current | Queued::Already) => {
          debug!(?target, "file of a batch analyzed or queued already");
        }
        Ok(Queued::Full) => {
          backlog.push_front(target);
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
