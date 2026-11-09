//! One download's task: it waits its turn in the local queue, checks its record, then tries
//! the download, waiting longer before each new try and for a slot before each, until the file
//! arrives, the download fails for good or it is canceled, and writes how it ended. A stop of
//! the daemon leaves the record as it is, for the stop to end.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use api_types::events::{DownloadFailureStage, DownloadFileFailed, Event};
use api_types::transfers::TransferStates;
use chrono::Utc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, info_span, warn};
use uuid::Uuid;

use super::Downloads;
use super::attempt::{Attempted, Failure};
use super::destination::incomplete_path;
use super::starts::Turn;
use crate::files::WriteGuard;
use crate::safety::Os;
use crate::sync::lock;
use crate::transfers::{Record, removal, store};
use reseek_relay_proto::backoff;

/// How long a download may hold its turn among the files of its request before its peer has
/// it queued.
const HARD_LIMIT: Duration = Duration::from_secs(180);

/// A download's turn among the files of its request, held from the start of its task until
/// its peer has it queued or it ends, and for [`HARD_LIMIT`] at most.
pub(super) struct Gate {
  _turn: Turn,
  /// when the download is stopped if it still holds the turn
  pub deadline: Instant,
}

/// How a download task ended.
enum Ended {
  /// the file arrived and is in the incomplete file, still marked as written; the record is
  /// filled in, to be written once the file is moved
  Succeeded(Record, PathBuf, WriteGuard),
  /// the download failed for good
  Failed(Failed),
  /// the daemon is stopping; the record is left as it is
  Abandoned,
}

/// A download that failed for good: its record when there is one, why, and its incomplete file.
struct Failed {
  record: Option<Record>,
  failure: Failure,
  incomplete: Option<PathBuf>,
}

impl Ended {
  fn failed(record: Option<Record>, failure: Failure, incomplete: Option<PathBuf>) -> Self {
    Self::Failed(Failed {
      record,
      failure,
      incomplete,
    })
  }
}

/// The wait before try `attempt`, the second or a later one: `delay`, doubled for every try
/// between the first and this one, plus the shared jitter, at most `max` in all.
fn retry_wait(attempt: u32, delay: Duration, max: Duration) -> Duration {
  backoff::wait(attempt - 1, backoff::doubling(delay), max).min(max)
}

impl Downloads {
  /// Starts the task of the download `record`, which waits at `position` in the local queue,
  /// takes a turn of `request`, whose files hold that many at once, and stops when `cancel` is
  /// canceled.
  pub(super) fn spawn(
    &self,
    record: Record,
    position: f64,
    request: (u64, usize),
    cancel: CancellationToken,
  ) {
    let this = self.this();
    let span = info_span!(
      "transfer",
      id = %record.id,
      direction = "download",
      username = record.username
    );
    self
      .tasks
      .spawn(async move { this.run(record.id, position, request, cancel).await }.instrument(span));
  }

  async fn run(
    self: Arc<Self>,
    id: Uuid,
    position: f64,
    request: (u64, usize),
    cancel: CancellationToken,
  ) {
    match self.download(id, position, request, &cancel).await {
      Ended::Succeeded(record, incomplete, writing) => {
        let this = self.clone();
        let moving = async move {
          this.complete(record, incomplete, writing).await;
          this.forget(id).await;
        };
        if self.spawn_move(moving.in_current_span()) {
          return;
        }
        // the file stays whole in the incomplete directory, where the resume after the next
        // start finds it arrived
        info!("download arrived after the stop's limit; its move is left to the next start");
      }
      Ended::Failed(failed) => self.fail(failed).await,
      Ended::Abandoned => debug!("download left for the stop to end"),
    }
    self.forget(id).await;
  }

  /// The download's tries, from its turn in the local queue to how it ended.
  async fn download(
    &self,
    id: Uuid,
    position: f64,
    request: (u64, usize),
    cancel: &CancellationToken,
  ) -> Ended {
    let first = tokio::select! {
      grant = self.starts.wait(id, position, Some(request)) => grant,
      () = cancel.cancelled() => return self.canceled(id, None).await,
    };
    let mut first_slot = Some(first.slot);
    let mut gate = first.turn.map(|turn| Gate {
      _turn: turn,
      deadline: Instant::now() + HARD_LIMIT,
    });
    let mut record = match self.db.read(move |conn| store::get(conn, id)).await {
      Ok(Some(record)) => record,
      Ok(None) => {
        return Ended::failed(
          None,
          Failure::terminal(format!("Transfer with ID {id} not found")),
          None,
        );
      }
      Err(e) => return Ended::failed(None, Failure::terminal(e.to_string()), None),
    };
    let waiting = TransferStates::Queued | TransferStates::Locally;
    if record.state != waiting {
      let message = format!(
        "Invalid starting state for download; expected {waiting}, encountered {}",
        record.state
      );
      return Ended::failed(Some(record), Failure::terminal(message), None);
    }
    let tracked = self
      .network
      .client()
      .downloads()
      .iter()
      .any(|t| t.username == record.username && t.filename.as_str() == record.filename);
    if tracked {
      let message = "A duplicate download of the same file to the same user is already registered";
      return Ended::failed(Some(record), Failure::terminal(message.to_string()), None);
    }
    let retry = self
      .config
      .current()
      .values
      .transfers
      .download
      .retry
      .clone();
    let incomplete = match incomplete_path(
      &self.incomplete,
      &record.username,
      &record.filename,
      Os::CURRENT,
    ) {
      Ok((path, shortened)) => {
        if shortened {
          warn!(path = %path.display(), "incomplete path too long; the remote directories are left out");
        }
        path
      }
      Err(e) => return Ended::failed(Some(record), Failure::terminal(e), None),
    };
    record.attempts = 1;
    self.save(&record, false).await;
    let mut attempt = 1;
    // the wait before the try about to start, none before the first
    let mut delay: Option<Duration> = None;
    loop {
      if let Some(delay) = delay.take() {
        info!(
          attempt,
          seconds = delay.as_secs_f64().ceil() as u64,
          "Waiting to retry the download"
        );
        tokio::select! {
          () = tokio::time::sleep(delay) => {}
          () = cancel.cancelled() => return self.canceled(id, Some(incomplete)).await,
        }
        record.next_attempt_at = None;
        self.save(&record, false).await;
      }
      // held for this try; a retry waits for one in its place in the queue, keeping its
      // request's turn
      let _slot = match first_slot.take() {
        Some(slot) => slot,
        None => tokio::select! {
          grant = self.starts.wait(id, position, None) => grant.slot,
          () = cancel.cancelled() => return self.canceled(id, Some(incomplete)).await,
        },
      };
      match self
        .attempt(&mut record, &incomplete, cancel, &mut gate)
        .await
      {
        Attempted::Succeeded(writing) => return Ended::Succeeded(record, incomplete, writing),
        Attempted::Abandoned => return Ended::Abandoned,
        Attempted::Failed(failure) => {
          warn!(
            attempt,
            filename = record.filename,
            state = %failure.state,
            reason = failure.message,
            "Failed attempt to download the file"
          );
          if !failure.retryable || attempt >= retry.attempts {
            return Ended::failed(Some(record), failure, Some(incomplete));
          }
          // waiting to retry: queued here again, with the reason it failed shown meanwhile and
          // no start until the next try starts
          attempt += 1;
          record.state = TransferStates::Queued | TransferStates::Locally;
          record.attempts = attempt;
          record.started_at = None;
          record.ended_at = None;
          record.place_in_queue = None;
          record.average_speed = 0.0;
          record.exception = Some(failure.message);
          let wait = retry_wait(attempt, retry.delay, retry.max_delay);
          record.next_attempt_at =
            Some(Utc::now() + chrono::Duration::from_std(wait).unwrap_or(chrono::Duration::MAX));
          delay = Some(wait);
          self.save(&record, true).await;
        }
      }
    }
  }

  /// The end of a download canceled while it was not trying: left for the stop when the
  /// daemon is stopping, else failed as canceled.
  async fn canceled(&self, id: Uuid, incomplete: Option<PathBuf>) -> Ended {
    if self.shutdown.is_cancelled() {
      return Ended::Abandoned;
    }
    match self.db.read(move |conn| store::get(conn, id)).await {
      Ok(record) => Ended::failed(record, Failure::canceled(), incomplete),
      Err(e) => {
        warn!(error = %e, "canceled download not read");
        Ended::failed(None, Failure::canceled(), incomplete)
      }
    }
  }

  /// Whether the download `id` is to leave the listings once it has ended.
  pub(super) fn removal_pending(&self, id: Uuid) -> bool {
    lock(&self.in_flight)
      .get(&id)
      .is_some_and(|entry| entry.remove)
  }

  /// Writes a download that failed for good as completed with its outcome, unless it has ended
  /// already, counts it as failed, and, unless it was canceled, raises `DownloadFileFailed`.
  async fn fail(&self, failed: Failed) {
    let Failed {
      record,
      failure,
      incomplete,
    } = failed;
    let Some(mut record) = record else {
      return warn!(reason = failure.message, "download without a record failed");
    };
    if record.is_completed() {
      return debug!(state = %record.state, "download ended already");
    }
    let canceled = failure.state == TransferStates::Cancelled;
    if canceled && self.pausing(record.id) {
      record.paused = true;
      record.state = TransferStates::Queued | TransferStates::Locally;
      record.started_at = None;
      record.ended_at = None;
      record.exception = None;
      record.next_attempt_at = None;
      record.place_in_queue = None;
      record.average_speed = 0.0;
      self.save(&record, true).await;
      return info!("download paused");
    }
    let reason = lock(&self.in_flight)
      .get(&record.id)
      .and_then(|entry| entry.reason);
    record.state = TransferStates::Completed | failure.state;
    record.ended_at = Some(record.ended_at.unwrap_or_else(Utc::now));
    record.exception = Some(match reason {
      Some(reason) if canceled => reason.to_string(),
      _ => failure.message,
    });
    record.next_attempt_at = None;
    record.place_in_queue = None;
    record.removed = self.removal_pending(record.id);
    self.save(&record, true).await;
    self.telemetry.metrics().transfers().downloads.failed();
    if canceled {
      return info!(reason = record.exception, "download canceled");
    }
    info!(state = %record.state, reason = record.exception, "download failed");
    let local_filename = match incomplete {
      // a file whose presence cannot be read is not named
      Some(path) if tokio::fs::try_exists(&path).await.unwrap_or(false) => {
        Some(path.display().to_string())
      }
      _ => None,
    };
    let ended_at = record.ended_at.unwrap_or_else(Utc::now);
    self.events.raise_at(
      Event::DownloadFileFailed(DownloadFileFailed {
        version: 0,
        stage: DownloadFailureStage::Download,
        reason: record.exception.clone().unwrap_or_default(),
        local_filename,
        remote_filename: record.filename.clone(),
        transfer: record.transfer(),
      }),
      ended_at,
    );
  }

  /// Lets go of the download `id` once its task has written how it ended, removing it from the
  /// listings when that was asked for meanwhile.
  async fn forget(&self, id: Uuid) {
    lock(&self.asked).remove(&id);
    self.starts.forget(id);
    let entry = lock(&self.in_flight).remove(&id);
    if entry.is_some_and(|entry| entry.remove) {
      let download = api_types::transfers::TransferDirection::Download;
      if let Err(e) = removal::remove(&self.db, &self.live, download, id).await {
        warn!(%id, error = %e, "ended download not removed");
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const SECOND: Duration = Duration::from_secs(1);

  #[test]
  fn each_retry_waits_twice_as_long_and_the_jitter_never_takes_it_past_the_most() {
    let whole = |attempt| retry_wait(attempt, 5 * SECOND, 60 * SECOND).as_secs();
    let waits: Vec<u64> = (2..=9).map(whole).collect();
    assert_eq!(
      waits,
      [5, 10, 20, 40, 60, 60, 60, 60],
      "under a second of jitter"
    );
    for _ in 0..100 {
      assert_eq!(retry_wait(9, 5 * SECOND, 60 * SECOND), 60 * SECOND);
    }
  }
}
