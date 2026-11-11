//! What the client keeps about one transfer while it runs, the snapshot it hands out, and how
//! progress and speed are worked out. Every state change and progress update goes through the
//! transfer's tracker, which updates the record, calls the caller's callback and raises the event,
//! in that order, on the transfer's task.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use soulseek_proto::{RemoteFilename, TransferDirection};
use tokio::time::Instant;

use super::options::{ProgressUpdated, StateChanged};
use super::state::{Refused, TransferState};
use crate::client::Client;
use crate::error::ClientError;
use crate::events::Event;
use crate::lock;

/// how often the moving average of the speed takes a new sample
const SPEED_SAMPLE: Duration = Duration::from_secs(1);
/// how much of each new sample the moving average takes in
const SPEED_SMOOTHING: f64 = 0.2;

/// A transfer as it was at one moment, handed to callbacks and events and returned by the
/// operations
#[derive(Clone, Debug)]
pub struct Transfer {
  /// which way the file moves, seen from us
  pub direction: TransferDirection,
  pub username: String,
  /// the file as the peer names it, never a local path
  pub filename: RemoteFilename,
  /// our token for the transfer
  pub token: u32,
  /// downloads: the token the uploader gave the transfer when it offered to start it
  pub remote_token: Option<u32>,
  pub state: TransferState,
  /// the size expected, in bytes
  pub size: u64,
  /// where the transfer starts in the file
  pub start_offset: u64,
  /// the position reached: the start offset plus the bytes moved
  pub bytes_transferred: u64,
  pub bytes_remaining: u64,
  pub percent_complete: f64,
  /// bytes/second; 0 for the first second
  pub average_speed: f64,
  /// when bytes started moving, or when the transfer ended if they never did
  pub start_time: Option<SystemTime>,
  pub end_time: Option<SystemTime>,
  /// from the start to the end, or to now while it runs
  pub elapsed_time: Option<Duration>,
  /// the bytes remaining at the average speed; unknown while the speed is 0
  pub remaining_time: Option<Duration>,
  /// the far end of the transfer connection, once there is one
  pub ip_end_point: Option<SocketAddr>,
  /// what ended the transfer, unless it succeeded
  pub error: Option<ClientError>,
}

/// The mutable record behind a transfer's snapshots
pub(crate) struct Record {
  direction: TransferDirection,
  username: String,
  filename: RemoteFilename,
  token: u32,
  remote_token: Option<u32>,
  state: TransferState,
  size: u64,
  start_offset: u64,
  bytes_transferred: u64,
  average_speed: f64,
  started: Option<(Instant, SystemTime)>,
  ended: Option<(Instant, SystemTime)>,
  /// the last sample the moving average took, and whether it took one yet
  sample: (Instant, u64),
  sampled: bool,
  ip_end_point: Option<SocketAddr>,
  error: Option<ClientError>,
}

impl Record {
  pub(crate) fn new(
    direction: TransferDirection,
    username: &str,
    filename: &RemoteFilename,
    token: u32,
    size: u64,
    start_offset: u64,
  ) -> Self {
    Self {
      direction,
      username: username.to_owned(),
      filename: filename.clone(),
      token,
      remote_token: None,
      state: TransferState::QueuedLocally,
      size,
      start_offset,
      bytes_transferred: start_offset,
      average_speed: 0.0,
      started: None,
      ended: None,
      sample: (Instant::now(), start_offset),
      sampled: false,
      ip_end_point: None,
      error: None,
    }
  }

  pub(crate) fn snapshot(&self) -> Transfer {
    let now = Instant::now();
    let bytes_remaining = self.size.saturating_sub(self.bytes_transferred);
    Transfer {
      direction: self.direction,
      username: self.username.clone(),
      filename: self.filename.clone(),
      token: self.token,
      remote_token: self.remote_token,
      state: self.state,
      size: self.size,
      start_offset: self.start_offset,
      bytes_transferred: self.bytes_transferred,
      bytes_remaining,
      percent_complete: if self.size > 0 {
        self.bytes_transferred as f64 / self.size as f64 * 100.0
      } else {
        0.0
      },
      average_speed: self.average_speed,
      start_time: self.started.map(|(_, at)| at),
      end_time: self.ended.map(|(_, at)| at),
      elapsed_time: self
        .started
        .map(|(started, _)| self.ended.map_or(now, |(ended, _)| ended) - started),
      // a size the peer named and a crawling speed can make this too long for a duration
      remaining_time: (self.average_speed > 0.0)
        .then(|| Duration::try_from_secs_f64(bytes_remaining as f64 / self.average_speed).ok())
        .flatten(),
      ip_end_point: self.ip_end_point,
      error: self.error.clone(),
    }
  }

  /// Moves to `state` when the state machine allows it, stamping the start on the state with
  /// bytes moving and the end on the completed one; a transfer that ends without bytes moving
  /// starts when it ends
  fn set_state(&mut self, state: TransferState, now: Instant) -> Result<(), Refused> {
    self.state = self.state.to(state)?;
    if state == TransferState::InProgress && self.started.is_none() {
      self.started = Some((now, SystemTime::now()));
      self.sample = (now, self.start_offset);
    }
    if state.is_terminal() && self.ended.is_none() {
      let ended = (now, SystemTime::now());
      self.ended = Some(ended);
      self.started.get_or_insert(ended);
      // the speed becomes the overall average
      self.set_progress(self.bytes_transferred, now);
    }
    Ok(())
  }

  /// Records the position `bytes`. The speed is the overall average once every byte moved or
  /// the transfer is over; while it runs it is a moving average sampled once a second.
  fn set_progress(&mut self, bytes: u64, now: Instant) {
    self.bytes_transferred = bytes;
    let Some((started, _)) = self.started else {
      return;
    };
    let moved = bytes.saturating_sub(self.start_offset) as f64;
    let overall = |until: Instant| {
      moved
        / (until - started)
          .max(Duration::from_millis(1))
          .as_secs_f64()
    };
    if let Some((ended, _)) = self.ended {
      self.average_speed = overall(ended);
    } else if bytes >= self.size {
      self.average_speed = overall(now);
    } else {
      let (at, at_bytes) = self.sample;
      let since = now - at;
      if since < SPEED_SAMPLE {
        return;
      }
      let instant = bytes.saturating_sub(at_bytes) as f64 / since.as_secs_f64();
      self.average_speed = if self.sampled {
        self.average_speed + SPEED_SMOOTHING * (instant - self.average_speed)
      } else {
        instant
      };
      self.sampled = true;
      self.sample = (now, bytes);
    }
  }
}

/// Reports one transfer's changes: updates its record, then calls the caller's callback, then
/// raises the event
#[derive(Clone)]
pub(crate) struct Tracker {
  client: Client,
  record: Arc<Mutex<Record>>,
  state_changed: Vec<StateChanged>,
  progress_updated: Option<ProgressUpdated>,
}

impl Tracker {
  pub(crate) fn new(
    client: Client,
    record: Arc<Mutex<Record>>,
    state_changed: Vec<StateChanged>,
    progress_updated: Option<ProgressUpdated>,
  ) -> Self {
    Self {
      client,
      record,
      state_changed,
      progress_updated,
    }
  }

  pub(crate) fn snapshot(&self) -> Transfer {
    lock(&self.record).snapshot()
  }

  pub(crate) fn state(&self) -> TransferState {
    lock(&self.record).state
  }

  /// the position reached
  pub(crate) fn bytes_transferred(&self) -> u64 {
    lock(&self.record).bytes_transferred
  }

  pub(crate) fn update(&self, change: impl FnOnce(&mut Record)) {
    change(&mut lock(&self.record));
  }

  pub(crate) fn set_size(&self, size: u64) {
    self.update(|record| record.size = size);
  }

  pub(crate) fn set_remote_token(&self, token: u32) {
    self.update(|record| record.remote_token = Some(token));
  }

  pub(crate) fn set_ip_end_point(&self, endpoint: SocketAddr) {
    self.update(|record| record.ip_end_point = Some(endpoint));
  }

  /// the transfer starts at `offset`, as the downloader told the uploader
  pub(crate) fn set_start_offset(&self, offset: u64) {
    self.update(|record| {
      record.start_offset = offset;
      record.bytes_transferred = offset;
    });
  }

  /// Reports the state a new transfer starts in, with no state before it
  pub(crate) fn emit_first_state(&self) {
    self.report(None, self.snapshot());
  }

  /// Moves the transfer to `state` and reports it; a terminal state carries `error`. A move the
  /// state machine does not allow leaves the transfer as it is and fails as an invalid
  /// operation.
  pub(crate) fn emit_state(
    &self,
    state: TransferState,
    error: Option<ClientError>,
  ) -> Result<(), ClientError> {
    let (previous, transfer) = {
      let mut record = lock(&self.record);
      let previous = record.state;
      record
        .set_state(state, Instant::now())
        .map_err(|refused| ClientError::InvalidOperation(refused.to_string()))?;
      record.error = error;
      (previous, record.snapshot())
    };
    tracing::debug!(%previous, %state, "transfer state changed");
    self.report(Some(previous), transfer);
    Ok(())
  }

  /// calls the callbacks with the state change, then raises its event
  fn report(&self, previous: Option<TransferState>, transfer: Transfer) {
    for callback in &self.state_changed {
      callback(previous, &transfer);
    }
    self.client.emit(Event::TransferStateChanged {
      previous_state: previous,
      transfer,
    });
  }

  /// records the position `bytes` and reports it
  pub(crate) fn emit_progress(&self, bytes: u64) {
    let (previous, transfer) = {
      let mut record = lock(&self.record);
      let previous = record.bytes_transferred;
      record.set_progress(bytes, Instant::now());
      (previous, record.snapshot())
    };
    tracing::trace!(bytes, "transfer progress");
    if let Some(callback) = &self.progress_updated {
      callback(previous, &transfer);
    }
    self.client.emit(Event::TransferProgressUpdated {
      previous_bytes_transferred: previous,
      transfer,
    });
  }
}

#[cfg(test)]
mod tests {
  use super::super::state::TransferOutcome;
  use super::*;

  fn record(size: u64, start_offset: u64) -> Record {
    Record::new(
      TransferDirection::Download,
      "bob",
      &"a.flac".into(),
      1,
      size,
      start_offset,
    )
  }

  /// moves `record` from Initializing to InProgress
  fn start(record: &mut Record) {
    record.state = TransferState::Initializing;
    record
      .set_state(TransferState::InProgress, Instant::now())
      .unwrap();
  }

  #[tokio::test(start_paused = true)]
  async fn speed_is_zero_for_the_first_second_then_a_moving_average() {
    let mut record = record(10_000, 0);
    start(&mut record);
    tokio::time::advance(Duration::from_millis(500)).await;
    record.set_progress(500, Instant::now());
    assert_eq!(record.average_speed, 0.0);
    assert!(record.snapshot().remaining_time.is_none());
    tokio::time::advance(Duration::from_millis(500)).await;
    record.set_progress(1000, Instant::now());
    assert_eq!(record.average_speed, 1000.0);
    tokio::time::advance(Duration::from_secs(1)).await;
    record.set_progress(3000, Instant::now());
    // 1000 + 0.2 * (2000 - 1000)
    assert_eq!(record.average_speed, 1200.0);
    assert_eq!(
      record.snapshot().remaining_time,
      Some(Duration::from_secs_f64(7000.0 / 1200.0))
    );
  }

  #[tokio::test(start_paused = true)]
  async fn completion_makes_the_speed_the_overall_average_of_the_bytes_moved() {
    let mut record = record(4000, 1000);
    start(&mut record);
    tokio::time::advance(Duration::from_millis(1500)).await;
    record.set_progress(4000, Instant::now());
    record
      .set_state(
        TransferState::Completed(TransferOutcome::Succeeded),
        Instant::now(),
      )
      .unwrap();
    let transfer = record.snapshot();
    assert_eq!(transfer.average_speed, 2000.0);
    assert_eq!(transfer.bytes_transferred, 4000);
    assert_eq!(transfer.bytes_remaining, 0);
    assert_eq!(transfer.percent_complete, 100.0);
    assert_eq!(transfer.elapsed_time, Some(Duration::from_millis(1500)));
  }

  #[tokio::test(start_paused = true)]
  async fn a_transfer_that_ends_before_moving_bytes_starts_when_it_ends() {
    let mut record = record(4000, 0);
    tokio::time::advance(Duration::from_secs(3)).await;
    record
      .set_state(
        TransferState::Completed(TransferOutcome::Cancelled),
        Instant::now(),
      )
      .unwrap();
    let transfer = record.snapshot();
    assert_eq!(transfer.start_time, transfer.end_time);
    assert_eq!(transfer.elapsed_time, Some(Duration::ZERO));
    assert_eq!(transfer.average_speed, 0.0);
  }

  #[tokio::test(start_paused = true)]
  async fn a_remaining_time_too_long_to_hold_is_unknown() {
    let mut record = record(u64::MAX, 0);
    start(&mut record);
    tokio::time::advance(Duration::from_secs(1000)).await;
    record.set_progress(1, Instant::now());
    assert!(record.average_speed > 0.0);
    assert_eq!(record.snapshot().remaining_time, None);
  }

  #[test]
  fn an_empty_file_is_zero_percent_complete() {
    assert_eq!(record(0, 0).snapshot().percent_complete, 0.0);
  }
}
