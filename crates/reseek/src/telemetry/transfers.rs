//! The transfer family of metrics, the same for uploads and downloads: the gauges of what is
//! queued and in progress, read from the transfers the client runs, and the counters of what
//! completed. The family is taken under one lock (`Catalog::transfers`), so the figures one
//! update changes are seen together.

use std::collections::HashSet;

use api_types::telemetry as figures;
use soulseek_client::{Transfer, TransferState};

use super::catalog::PREFIX;
use super::derived::Ema;
use super::registry::{Counter, Gauge, Histogram, Registry};

/// Transfer speeds, in bytes per second: 1 KiB/s to 1 GiB/s.
fn speeds() -> Vec<f64> {
  vec![
    1024.0,
    10240.0,
    51200.0,
    102_400.0,
    204_800.0,
    307_200.0,
    409_600.0,
    512_000.0,
    1_048_576.0,
    2_097_152.0,
    4_194_304.0,
    6_291_456.0,
    10_485_760.0,
    26_214_400.0,
    52_428_800.0,
    104_857_600.0,
    262_144_000.0,
    524_288_000.0,
    1_073_741_824.0,
  ]
}

/// The transfers of one direction.
#[derive(Debug)]
pub struct TransferMetrics {
  /// users with queued transfers
  pub queued_users: Gauge,
  /// queued transfers
  pub queued_files: Gauge,
  /// the size of the queued transfers in bytes
  pub queued_bytes: Gauge,
  /// users with transfers in progress
  pub in_progress_users: Gauge,
  /// transfers in progress
  pub in_progress_files: Gauge,
  /// the size of the transfers in progress in bytes
  pub in_progress_bytes: Gauge,
  /// the moving average of the summed speed of the transfers in progress
  pub in_progress_speed_total: Ema,
  /// the moving average of the mean speed of the transfers in progress
  pub in_progress_speed_average: Ema,
  /// transfers that succeeded
  pub completed_succeeded: Counter,
  /// bytes moved by the transfers that succeeded
  pub completed_bytes: Counter,
  /// transfers that failed in any way
  pub completed_failed: Counter,
  /// the average speeds of the transfers that succeeded
  pub completed_speed_average: Histogram,
  /// the moving average of `completed_speed_average`
  pub completed_speed_average_current: Ema,
}

/// Both directions.
#[derive(Debug)]
pub struct TransferFamily {
  /// uploads
  pub uploads: TransferMetrics,
  /// downloads
  pub downloads: TransferMetrics,
}

impl TransferFamily {
  /// The figures of both directions as the event stream carries them, taken together.
  pub fn figures(&self) -> figures::TransferMetrics {
    figures::TransferMetrics {
      downloads: self.downloads.figures(),
      uploads: self.uploads.figures(),
    }
  }
}

/// Whether a transfer in `state` waits: requested, or queued here or at the peer.
fn queued(state: TransferState) -> bool {
  state == TransferState::REQUESTED || state.contains(TransferState::QUEUED)
}

/// Whether a transfer in `state` is under way: making its connection or moving bytes.
fn in_progress(state: TransferState) -> bool {
  state == TransferState::INITIALIZING || state == TransferState::IN_PROGRESS
}

impl TransferMetrics {
  /// The metrics of `dir`, `uploads` or `downloads`, registered in `r` at zero.
  pub(super) fn register(r: &mut Registry, dir: &str) -> Self {
    let name = |rest: &str| format!("{PREFIX}_transfers_{dir}_{rest}");
    Self {
      queued_users: r.gauge(
        name("queued_users_current"),
        format!("Current number of unique users with queued {dir}"),
      ),
      queued_files: r.gauge(
        name("queued_files_current"),
        format!("Current number of queued {dir}"),
      ),
      queued_bytes: r.gauge(
        name("queued_bytes_current"),
        format!("Current total size, in bytes, of queued {dir}"),
      ),
      in_progress_users: r.gauge(
        name("in_progress_users_current"),
        format!("Current number of unique users with in-progress {dir}"),
      ),
      in_progress_files: r.gauge(
        name("in_progress_files_current"),
        format!("Current number of in-progress {dir}"),
      ),
      in_progress_bytes: r.gauge(
        name("in_progress_bytes_current"),
        format!("Current total size, in bytes, of in-progress {dir}"),
      ),
      in_progress_speed_total: Ema::new(r.gauge(
        name("in_progress_speed_total_current"),
        format!("Current total speed, in bytes per second, of in-progress {dir}"),
      )),
      in_progress_speed_average: Ema::new(r.gauge(
        name("in_progress_speed_average_current"),
        format!("Current average speed, in bytes per second, of in-progress {dir}"),
      )),
      completed_succeeded: r.counter(
        name("completed_succeeded"),
        format!("Total number of {dir} that completed successfully"),
      ),
      completed_bytes: r.counter(
        name("completed_bytes"),
        format!("Total number of bytes transferred by successfully completed {dir}"),
      ),
      completed_failed: r.counter(
        name("completed_failed"),
        format!("Total number of {dir} that failed"),
      ),
      completed_speed_average: r.histogram(
        name("completed_speed_average"),
        format!("The average speed of completed {dir}, in bytes per second"),
        speeds(),
      ),
      completed_speed_average_current: Ema::new(r.gauge(
        name("completed_speed_average_current"),
        format!("Current average speed, in bytes per second, of completed {dir}"),
      )),
    }
  }

  /// A transfer that succeeded, having moved `bytes` at `speed` bytes per second on average.
  pub fn succeeded(&self, bytes: u64, speed: f64) {
    self.completed_succeeded.inc();
    self.completed_bytes.inc_by(bytes as f64);
    self.completed_speed_average.observe(speed);
    self.completed_speed_average_current.update(speed);
  }

  /// A transfer that failed in any way, canceled included.
  pub fn failed(&self) {
    self.completed_failed.inc();
  }

  /// The queued and in-progress gauges from `transfers`, the client's transfers of this
  /// direction now.
  pub fn count(&self, transfers: &[Transfer]) {
    let tally = |pick: fn(TransferState) -> bool| {
      let picked: Vec<&Transfer> = transfers.iter().filter(|t| pick(t.state)).collect();
      let users: HashSet<&str> = picked.iter().map(|t| t.username.as_str()).collect();
      let bytes: u64 = picked.iter().map(|t| t.size).sum();
      (users.len() as f64, picked.len() as f64, bytes as f64)
    };
    let (users, files, bytes) = tally(queued);
    self.queued_users.set(users);
    self.queued_files.set(files);
    self.queued_bytes.set(bytes);
    let (users, files, bytes) = tally(in_progress);
    self.in_progress_users.set(users);
    self.in_progress_files.set(files);
    self.in_progress_bytes.set(bytes);
  }

  /// The queued and in-progress gauges from counts made elsewhere, each users, files and
  /// bytes.
  pub fn set_counts(&self, queued: (u64, u64, u64), in_progress: (u64, u64, u64)) {
    let (users, files, bytes) = queued;
    self.queued_users.set(users as f64);
    self.queued_files.set(files as f64);
    self.queued_bytes.set(bytes as f64);
    let (users, files, bytes) = in_progress;
    self.in_progress_users.set(users as f64);
    self.in_progress_files.set(files as f64);
    self.in_progress_bytes.set(bytes as f64);
  }

  /// The figures of this direction as the event stream carries them.
  pub fn figures(&self) -> figures::DirectionMetrics {
    // gauges and counters hold whole counts
    let count = |value: f64| value as u64;
    figures::DirectionMetrics {
      in_progress: figures::InProgressMetrics {
        files: count(self.in_progress_files.get()),
        users: count(self.in_progress_users.get()),
        average_speed: self.in_progress_speed_average.current(),
        total_speed: self.in_progress_speed_total.current(),
      },
      queued: figures::QueuedMetrics {
        files: count(self.queued_files.get()),
        users: count(self.queued_users.get()),
        bytes: count(self.queued_bytes.get()),
      },
      completed: figures::CompletedMetrics {
        succeeded: count(self.completed_succeeded.get()),
        failed: count(self.completed_failed.get()),
        bytes: count(self.completed_bytes.get()),
      },
    }
  }

  /// Moves the speed averages toward the summed and the mean speed of the transfers in
  /// progress among `transfers`, or empties them when none is.
  pub fn pace(&self, transfers: &[Transfer]) {
    let speeds: Vec<f64> = transfers
      .iter()
      .filter(|t| t.state == TransferState::IN_PROGRESS)
      .map(|t| t.average_speed)
      .collect();
    if speeds.is_empty() {
      self.in_progress_speed_total.reset();
      self.in_progress_speed_average.reset();
      return;
    }
    let total: f64 = speeds.iter().sum();
    self.in_progress_speed_total.update(total);
    self
      .in_progress_speed_average
      .update(total / speeds.len() as f64);
  }
}
