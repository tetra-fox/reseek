//! One try of a download: where in the incomplete file it starts, the client's download with
//! each state written as it comes and the progress at most every [`PROGRESS_EVERY`], and what
//! its outcome means for the record.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use api_types::live::LiveFrame;
use api_types::transfers::TransferStates;
use chrono::Utc;
use soulseek_client::{
  ClientError, DownloadOutput, DownloadRequest, DownloadSink, LogLimiter, OutputFactory,
  ProgressUpdated, SizeMismatchResolver, StateChanged, Transfer as Snapshot, TransferOptions,
};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

use super::Downloads;
use super::destination::earlier_incomplete_path;
use super::task::Gate;
use crate::config::enums::RetryPartial;
use crate::files::mode::to_unix_file_mode;
use crate::files::{Files, WriteGuard, create_file};
use crate::network::options::request_style;
use crate::safety::Os;
use crate::transfers::{Record, store};

/// More states than one try reports.
const STATES: usize = 16;

/// How often a download's progress is written and pushed at most.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// The smallest size an old 32-bit client reports as 0, one byte short of 2 GiB.
const OVERFLOWED_SIZE: u64 = i32::MAX as u64;

/// Why a download that held its turn too long failed.
const HARD_LIMITED: &str = "Download failed to enqueue remotely after hard time limit of 180 secs";

/// Progress not written, which a failing database would repeat four times a second.
static PROGRESS_UNWRITTEN: LogLimiter = LogLimiter::new();

/// How a try ended.
pub(super) enum Attempted {
  /// the file arrived; the record is filled in as succeeded, to be written once the file is
  /// moved, and the incomplete file stays marked as written until then
  Succeeded(WriteGuard),
  /// the try failed
  Failed(Failure),
  /// the daemon is stopping
  Abandoned,
}

/// Why a try failed: the outcome it ends in, the reason shown, and whether trying again can
/// help.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Failure {
  /// the outcome flag: `Cancelled`, `TimedOut`, `Errored`, `Rejected` or `Aborted`
  pub state: TransferStates,
  /// the reason
  pub message: String,
  /// whether another try can succeed
  pub retryable: bool,
}

impl Failure {
  /// An error trying again does not mend.
  pub fn terminal(message: String) -> Self {
    Self {
      state: TransferStates::Errored,
      message,
      retryable: false,
    }
  }

  /// A cancel.
  pub fn canceled() -> Self {
    Self {
      state: TransferStates::Cancelled,
      message: "Operation cancelled".to_string(),
      retryable: false,
    }
  }
}

/// Whether an I/O error is one trying again does not mend: a full disk, a refused permission,
/// a read-only filesystem.
fn lasting(error: &io::Error) -> bool {
  matches!(
    error.kind(),
    io::ErrorKind::StorageFull
      | io::ErrorKind::PermissionDenied
      | io::ErrorKind::ReadOnlyFilesystem
  )
}

/// What a client error ending a try means for the download. The peer's refusal, a size
/// dispute, a cancel and a duplicate end it; a timeout, a lost connection, the peer's failure
/// or being offline are tried again, as is a local I/O error that is not lasting.
pub(super) fn failure(error: &ClientError) -> Failure {
  let retry = |state, message: String| Failure {
    state,
    message,
    retryable: true,
  };
  match error {
    ClientError::Canceled => Failure::canceled(),
    ClientError::Timeout(message) => retry(TransferStates::TimedOut, message.clone()),
    ClientError::TransferRejected(reason) => Failure {
      state: TransferStates::Rejected,
      message: reason.clone(),
      retryable: false,
    },
    ClientError::SizeMismatch { local, remote, .. } => Failure {
      state: TransferStates::Aborted,
      message: format!("the remote size of {remote} does not match expected size {local}"),
      retryable: false,
    },
    ClientError::DuplicateTransfer { .. } => Failure::terminal(
      "A duplicate download of the same file to the same user is already registered".to_string(),
    ),
    ClientError::Io { message, source } => Failure {
      state: TransferStates::Errored,
      message: message.clone(),
      retryable: !source
        .as_deref()
        .and_then(|source| source.downcast_ref::<io::Error>())
        .is_some_and(lasting),
    },
    other => retry(TransferStates::Errored, other.to_string()),
  }
}

/// Settles a size the peer reports differently from ours: the peer's is taken when no partial
/// file was resumed, else the download is aborted, the partial left for a later try with the
/// size a fresh search gives. The 0 an old client reports for a file of `OVERFLOWED_SIZE`
/// bytes or more keeps ours.
fn peers_size() -> SizeMismatchResolver {
  Arc::new(|transfer, remote| {
    if remote == 0 && transfer.size >= OVERFLOWED_SIZE {
      return Ok(transfer.size);
    }
    if transfer.start_offset > 0 {
      return Err(ClientError::transfer(
        "a partial file of the size asked for was resumed",
      ));
    }
    Ok(remote)
  })
}

/// Where a try starts in the incomplete file.
#[derive(Debug, PartialEq)]
enum Start {
  /// this many bytes in
  At(u64),
  /// nowhere: the file is whole, every byte having arrived before a stop that came ahead of
  /// its move
  Whole,
}

/// Where a try starts: at the length of a partial file with `partial` set to resume, else at
/// 0. A file as long as `size` is whole; one longer, from a file that changed, is deleted.
async fn start_offset(incomplete: &Path, size: u64, partial: RetryPartial) -> io::Result<Start> {
  let length = match tokio::fs::metadata(incomplete).await {
    Ok(metadata) => metadata.len(),
    Err(e) if e.kind() == io::ErrorKind::NotFound => {
      // a link whose target is gone is an error, not a missing file
      if tokio::fs::symlink_metadata(incomplete).await.is_ok() {
        return Err(io::Error::new(
          io::ErrorKind::NotFound,
          "the incomplete file is a link to nothing",
        ));
      }
      return Ok(Start::At(0));
    }
    Err(e) => return Err(e),
  };
  if length == 0 {
    return Ok(Start::At(0));
  }
  if length == size {
    info!(size, "incomplete file already whole");
    return Ok(Start::Whole);
  }
  if length > size {
    warn!(
      length,
      size, "partial download longer than the file; starting again"
    );
    tokio::fs::remove_file(incomplete).await?;
    return Ok(Start::At(0));
  }
  match partial {
    RetryPartial::Resume => {
      info!(offset = length, "Resuming download");
      Ok(Start::At(length))
    }
    RetryPartial::Overwrite => {
      info!(length, "Overwriting partial download");
      Ok(Start::At(0))
    }
  }
}

/// Whether anything is at `path`, a link to nothing included.
fn present(path: &Path) -> io::Result<bool> {
  match std::fs::symlink_metadata(path) {
    Ok(_) => Ok(true),
    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
    Err(e) => Err(e),
  }
}

/// Renames the partial file at `earlier`, where incomplete files were put before their names
/// carried a key, to `incomplete`, which the caller has marked, so a download begun then
/// resumes. Returns whether there was one to take; one another download writes is left to it.
// TODO: remove once no partial file named the earlier way is left to resume
fn rename_earlier(files: &Files, earlier: &Path, incomplete: &Path) -> io::Result<bool> {
  if present(incomplete)? || !present(earlier)? {
    return Ok(false);
  }
  // refused only while another download writes it
  let Ok(_earlier) = files.writing(earlier) else {
    return Ok(false);
  };
  std::fs::rename(earlier, incomplete)?;
  Ok(true)
}

/// Opens the incomplete file at `path` for a try starting at `offset`, once the peer is ready
/// to send: made with `mode` when new, cut at `offset`.
fn output(path: PathBuf, mode: Option<u32>, offset: u64) -> OutputFactory {
  Box::new(move || {
    Box::pin(async move {
      let file = tokio::task::spawn_blocking(move || create_file(&path, mode, offset))
        .await
        .map_err(io::Error::other)??;
      Ok(Box::new(tokio::fs::File::from_std(file)) as Box<dyn DownloadSink>)
    })
  })
}

impl Downloads {
  /// One try of the download `record` into `incomplete`, stopped by `cancel`, or by the
  /// request's hard limit while it holds `gate`.
  pub(super) async fn attempt(
    &self,
    record: &mut Record,
    incomplete: &Path,
    cancel: &CancellationToken,
    gate: &mut Option<Gate>,
  ) -> Attempted {
    let options = self.config.current().values.clone();
    let download = &options.transfers.download;
    // marked before its length is read, so nothing deletes the partial file in between
    let files = self.files.clone();
    let marked = incomplete.to_path_buf();
    let writing = match tokio::task::spawn_blocking(move || files.writing(&marked)).await {
      Ok(Ok(guard)) => guard,
      // two downloads whose names give one incomplete file; this one waits for its next try
      Ok(Err(e)) => {
        return Attempted::Failed(Failure {
          state: TransferStates::Errored,
          message: format!("Failed to start the download: {e}"),
          retryable: true,
        });
      }
      Err(e) => {
        return Attempted::Failed(Failure::terminal(format!(
          "the incomplete file was not marked: {e}"
        )));
      }
    };
    if let Err(e) = self.adopt_earlier(record, incomplete).await {
      return Attempted::Failed(Failure {
        state: TransferStates::Errored,
        message: format!("Failed to take over the partial file: {e}"),
        retryable: !lasting(&e),
      });
    }
    let start = match start_offset(incomplete, record.size, download.retry.partial).await {
      Ok(Start::At(start)) => start,
      Ok(Start::Whole) => {
        record.state = TransferStates::Completed | TransferStates::Succeeded;
        record.bytes_transferred = record.size;
        record.ended_at = Some(Utc::now());
        record.exception = None;
        record.place_in_queue = None;
        return Attempted::Succeeded(writing);
      }
      Err(e) => {
        return Attempted::Failed(Failure {
          state: TransferStates::Errored,
          message: format!("Failed to read the partial file: {e}"),
          retryable: !lasting(&e),
        });
      }
    };
    // validated when the options were loaded
    let mode = download
      .destination
      .permissions
      .mode
      .as_deref()
      .and_then(|text| to_unix_file_mode(text).ok());
    let (states_sent, mut states) = mpsc::channel(STATES);
    let state_changed: StateChanged = Arc::new(move |_, snapshot: &Snapshot| {
      if let Err(e) = states_sent.try_send(snapshot.clone()) {
        warn!(error = %e, "download state not recorded");
      }
    });
    let (progress_sent, mut progress) = watch::channel((start, 0.0));
    let progress_updated: ProgressUpdated = Arc::new(move |_, snapshot: &Snapshot| {
      progress_sent.send_replace((snapshot.bytes_transferred, snapshot.average_speed));
    });
    let request = DownloadRequest {
      username: record.username.clone(),
      filename: record.filename.clone().into(),
      size: record.size,
      start_offset: start,
      token: None,
      output: DownloadOutput::Stream(output(incomplete.to_path_buf(), mode, start)),
      options: TransferOptions {
        state_changed: Some(state_changed),
        progress_updated: Some(progress_updated),
        size_mismatch_resolver: Some(peers_size()),
        request_style: request_style(&options),
        ..TransferOptions::default()
      },
    };
    let stop = cancel.child_token();
    let client = self.network.client().clone();
    let running = client.download(request, &stop);
    tokio::pin!(running);
    // a turn the download no longer holds never fires
    let deadline = tokio::time::sleep_until(gate.as_ref().map_or_else(
      || Instant::now() + Duration::from_secs(86_400),
      |g| g.deadline,
    ));
    tokio::pin!(deadline);
    let throttle = tokio::time::sleep(Duration::ZERO);
    tokio::pin!(throttle);
    let mut throttled = false;
    let mut hard_limited = false;
    let mut ended: Option<Snapshot> = None;
    let result = loop {
      tokio::select! {
        biased;
        Some(snapshot) = states.recv() => self.state_changed(record, snapshot, gate, &mut ended).await,
        result = &mut running => break result,
        () = &mut deadline, if gate.is_some() => {
          let history = self.history(record.id).await;
          warn!(
            state = %record.state,
            history,
            "download not queued by the peer within the time limit"
          );
          hard_limited = true;
          gate.take();
          stop.cancel();
        }
        Ok(()) = progress.changed(), if !throttled => {
          let (bytes, speed) = *progress.borrow_and_update();
          self.progressed(record, bytes, speed).await;
          throttled = true;
          throttle.as_mut().reset(Instant::now() + PROGRESS_EVERY);
        }
        () = &mut throttle, if throttled => throttled = false,
      }
    };
    // the states the client reported before its task ended are all queued by now
    while let Ok(snapshot) = states.try_recv() {
      self.state_changed(record, snapshot, gate, &mut ended).await;
    }
    gate.take();
    if self.shutdown.is_cancelled() {
      return Attempted::Abandoned;
    }
    match result {
      Ok(transfer) => {
        record.take_figures(&transfer);
        record.state = TransferStates::Completed | TransferStates::Succeeded;
        record.size = transfer.size;
        record.exception = None;
        record.place_in_queue = None;
        // written once the file is moved, so no reader sees it succeeded without its path
        self
          .telemetry
          .metrics()
          .transfers()
          .downloads
          .succeeded(record.bytes_transferred, record.average_speed);
        info!(
          bytes = record.bytes_transferred,
          speed = record.average_speed,
          "download completed"
        );
        Attempted::Succeeded(writing)
      }
      Err(error) => {
        if let Some(snapshot) = &ended {
          record.take_figures(snapshot);
        }
        debug!(%error, "download try ended");
        Attempted::Failed(if hard_limited {
          Failure {
            state: TransferStates::TimedOut,
            message: HARD_LIMITED.to_string(),
            retryable: true,
          }
        } else {
          failure(&error)
        })
      }
    }
  }

  /// Moves a partial file of `record` named the earlier way to `incomplete`, marked already.
  async fn adopt_earlier(&self, record: &Record, incomplete: &Path) -> io::Result<()> {
    let earlier = earlier_incomplete_path(
      &self.incomplete,
      &record.username,
      &record.filename,
      Os::CURRENT,
    )
    .map_err(io::Error::other)?;
    let files = self.files.clone();
    let (from, to) = (earlier.clone(), incomplete.to_path_buf());
    let adopted = tokio::task::spawn_blocking(move || rename_earlier(&files, &from, &to))
      .await
      .map_err(io::Error::other)??;
    if adopted {
      info!(from = %earlier.display(), "partial file named the earlier way taken over");
    }
    Ok(())
  }

  /// Takes a state the client reported into `record` and writes it; a final state is kept in
  /// `ended` for the task to write with the outcome. A state past the peer's queue ends the
  /// download's turn in `gate`.
  async fn state_changed(
    &self,
    record: &mut Record,
    snapshot: Snapshot,
    gate: &mut Option<Gate>,
    ended: &mut Option<Snapshot>,
  ) {
    let state = TransferStates::from_bits_retain(snapshot.state.bits());
    let remotely = TransferStates::Queued | TransferStates::Remotely;
    let past_the_queue =
      TransferStates::Initializing | TransferStates::InProgress | TransferStates::Completed;
    if state == remotely || state.intersects(past_the_queue) {
      gate.take();
    }
    if state.contains(TransferStates::Completed) {
      *ended = Some(snapshot);
      return;
    }
    // records are left as they are once the daemon stops
    if self.shutdown.is_cancelled() {
      return;
    }
    let previous = record.state;
    record.take_figures(&snapshot);
    record.state = state;
    record.exception = None;
    if state == TransferStates::Initializing && previous != state {
      // the server never pushes what a user shares
      self.users.refetch_statistics(&record.username);
    }
    if snapshot.size != record.size {
      info!(
        asked = record.size,
        peer = snapshot.size,
        "download takes the peer's size"
      );
      record.size = snapshot.size;
    }
    if state == remotely {
      record.enqueued_at.get_or_insert_with(Utc::now);
    } else if previous == remotely {
      record.place_in_queue = None;
    }
    if state == TransferStates::InProgress && previous != state {
      info!(
        start = snapshot.start_offset,
        size = record.size,
        "download started"
      );
    }
    self.save(record, previous != state).await;
  }

  /// The states the download `id` entered, oldest first with when, as one line for a log.
  async fn history(&self, id: uuid::Uuid) -> String {
    match self.db.read(move |conn| store::history(conn, id)).await {
      Ok(states) => states
        .iter()
        .map(|change| format!("{} at {}", change.state, change.at.to_rfc3339()))
        .collect::<Vec<_>>()
        .join("; "),
      Err(e) => format!("not read: {e}"),
    }
  }

  /// Writes and pushes the bytes moved and the speed of a download in progress.
  async fn progressed(&self, record: &mut Record, bytes: u64, speed: f64) {
    if record.state != TransferStates::InProgress || self.shutdown.is_cancelled() {
      return;
    }
    record.bytes_transferred = bytes;
    record.average_speed = speed;
    let id = record.id;
    match self
      .db
      .write(move |conn| store::progress(conn, id, bytes, speed))
      .await
    {
      Ok(true) => {
        trace!(bytes, speed, "download progress");
        self
          .live
          .publish(LiveFrame::TransfersUpdated(Box::new(record.transfer())));
      }
      // the record left `InProgress` in the database meanwhile
      Ok(false) => {}
      Err(e) => {
        if let Some(suppressed) = PROGRESS_UNWRITTEN.allow() {
          warn!(error = %e, suppressed, "download progress not written");
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use soulseek_client::ConnectionErrorKind;

  use super::*;

  #[test]
  fn a_refused_or_duplicate_download_is_not_tried_again_and_a_lost_peer_is() {
    let cases = [
      (
        ClientError::TransferRejected("File not shared.".into()),
        false,
      ),
      (
        ClientError::DuplicateTransfer {
          username: "bob".into(),
          filename: "a".into(),
        },
        false,
      ),
      (ClientError::UserOffline("bob".into()), true),
      (
        ClientError::connection(ConnectionErrorKind::Read, "Remote connection closed"),
        true,
      ),
    ];
    for (error, retryable) in cases {
      assert_eq!(failure(&error).retryable, retryable, "{error}");
    }
  }

  #[test]
  fn a_lasting_disk_error_is_not_tried_again() {
    let full = ClientError::io(
      "Failed to write the download: disk full",
      io::Error::new(io::ErrorKind::StorageFull, "disk full"),
    );
    let failure = failure(&full);
    assert_eq!(failure.message, "Failed to write the download: disk full");
    assert!(!failure.retryable);
    let passing = ClientError::io("Failed to write", io::Error::other("interrupted"));
    assert!(super::failure(&passing).retryable);
  }

  fn snapshot(size: u64, start_offset: u64) -> Snapshot {
    Snapshot {
      direction: soulseek_proto::TransferDirection::Download,
      username: "bob".into(),
      filename: "a.flac".into(),
      token: 1,
      remote_token: None,
      state: soulseek_client::TransferState::QUEUED_REMOTELY,
      size,
      start_offset,
      bytes_transferred: start_offset,
      bytes_remaining: size - start_offset,
      percent_complete: 0.0,
      average_speed: 0.0,
      start_time: None,
      end_time: None,
      elapsed_time: None,
      remaining_time: None,
      ip_end_point: None,
      error: None,
    }
  }

  #[test]
  fn the_peers_size_is_taken_only_without_a_partial() {
    let resolve = peers_size();
    assert_eq!(resolve(&snapshot(100, 0), 120).unwrap(), 120);
    assert!(resolve(&snapshot(100, 40), 120).is_err());
    assert_eq!(
      resolve(&snapshot(OVERFLOWED_SIZE, 0), 0).unwrap(),
      OVERFLOWED_SIZE,
      "an old client's 0 for a large file"
    );
    assert_eq!(resolve(&snapshot(OVERFLOWED_SIZE - 1, 0), 0).unwrap(), 0);
  }

  #[tokio::test]
  async fn a_partial_resumes_or_starts_over_and_one_too_long_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.flac");
    let start = |partial| start_offset(&path, 10, partial);
    assert_eq!(start(RetryPartial::Resume).await.unwrap(), Start::At(0));
    std::fs::write(&path, b"1234").unwrap();
    assert_eq!(start(RetryPartial::Resume).await.unwrap(), Start::At(4));
    assert_eq!(start(RetryPartial::Overwrite).await.unwrap(), Start::At(0));
    std::fs::write(&path, b"0123456789").unwrap();
    assert_eq!(start(RetryPartial::Overwrite).await.unwrap(), Start::Whole);
    assert!(path.exists(), "a whole file is kept");
    std::fs::write(&path, b"0123456789a").unwrap();
    assert_eq!(start(RetryPartial::Resume).await.unwrap(), Start::At(0));
    assert!(
      !path.exists(),
      "a file longer than the download is not trusted"
    );
    #[cfg(unix)]
    {
      let link = dir.path().join("link.flac");
      std::os::unix::fs::symlink(dir.path().join("gone"), &link).unwrap();
      assert!(start_offset(&link, 10, RetryPartial::Resume).await.is_err());
    }
  }
}
