//! Reports over the transfer history in `transfers.db`: totals by outcome, a histogram over
//! time, rankings of users and of our shared directories, and the failures. Every report counts
//! a transfer by when it ended, removed ones included.
//!
//! A window includes both its ends, except the histogram's, whose end is excluded so a
//! transfer that ended on it is neither counted nor lost between buckets.

mod failures;
mod histogram;
mod rankings;
mod summary;

pub use histogram::{HistogramError, Width};

use api_types::reports::{TransferOutcome, TransferSummary};
use api_types::transfers::TransferDirection;
use chrono::{DateTime, Utc};
use rusqlite::types::Value;

use crate::db::{self, Database};
use crate::transfers::direction_text;

/// The first release; no transfer can have ended before it.
pub const GENESIS: &str = "2020-12-30T06:22:00Z";

/// [`GENESIS`] as a time.
pub fn genesis() -> DateTime<Utc> {
  GENESIS.parse().expect("the first release is RFC 3339")
}

/// The window a report covers, its ends as the API filled them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
  /// the start
  pub start: DateTime<Utc>,
  /// the end
  pub end: DateTime<Utc>,
}

/// The aggregate columns a [`TransferSummary`] is read from, at the start of a row.
const SUMMARY: &str = "COALESCE(SUM(Size), 0), COUNT(*), COUNT(DISTINCT Username), \
                       COALESCE(AVG(AverageSpeed), 0), \
                       COALESCE(AVG(unixepoch(StartedAt) - unixepoch(EnqueuedAt)), 0), \
                       COALESCE(AVG(unixepoch(EndedAt) - unixepoch(StartedAt)), 0)";

/// The reports, read from `transfers.db`.
pub struct Reports {
  db: Database,
}

impl Reports {
  /// The reports over `db`.
  pub fn new(db: Database) -> Self {
    Self { db }
  }
}

/// How a finished transfer in the state `state` ended: the state without `Completed`.
fn outcome(state: u32) -> TransferOutcome {
  match state & !16 {
    32 => TransferOutcome::Succeeded,
    64 => TransferOutcome::Cancelled,
    128 => TransferOutcome::TimedOut,
    256 => TransferOutcome::Errored,
    512 => TransferOutcome::Rejected,
    1024 => TransferOutcome::Aborted,
    _ => TransferOutcome::None,
  }
}

/// The [`TransferSummary`] of the [`SUMMARY`] columns starting at `at`.
fn summary_at(row: &rusqlite::Row, at: usize) -> rusqlite::Result<TransferSummary> {
  Ok(TransferSummary {
    total_bytes: row.get::<_, i64>(at)?.max(0).unsigned_abs(),
    count: row.get::<_, i64>(at + 1)?.max(0).unsigned_abs(),
    distinct_users: row.get::<_, i64>(at + 2)?.max(0).unsigned_abs(),
    average_speed: row.get(at + 3)?,
    average_wait: row.get(at + 4)?,
    average_duration: row.get(at + 5)?,
  })
}

/// A `WHERE` clause built a condition at a time, with its values.
#[derive(Default)]
struct Filter {
  conditions: Vec<String>,
  values: Vec<Value>,
}

impl Filter {
  /// Transfers that ended in `window`, both ends included.
  fn ended_between(mut self, window: Window) -> Self {
    self
      .conditions
      .push("EndedAt IS NOT NULL AND EndedAt BETWEEN ? AND ?".to_string());
    self.values.push(Value::Text(db::timestamp(window.start)));
    self.values.push(Value::Text(db::timestamp(window.end)));
    self
  }

  /// Transfers that ended at `window`'s start or later and before its end.
  fn ended_from(mut self, window: Window) -> Self {
    self
      .conditions
      .push("EndedAt IS NOT NULL AND EndedAt >= ? AND EndedAt < ?".to_string());
    self.values.push(Value::Text(db::timestamp(window.start)));
    self.values.push(Value::Text(db::timestamp(window.end)));
    self
  }

  /// Transfers in the state category whose condition is `category`.
  fn states(mut self, category: &str) -> Self {
    self.conditions.push(category.to_string());
    self
  }

  /// Transfers going `direction`, or either way when `None`.
  fn direction(mut self, direction: Option<TransferDirection>) -> Self {
    if let Some(direction) = direction {
      self.conditions.push("Direction = ?".to_string());
      self
        .values
        .push(Value::Text(direction_text(direction).to_string()));
    }
    self
  }

  /// Transfers with `username`, or with anyone when `None`.
  fn username(mut self, username: Option<&str>) -> Self {
    if let Some(username) = username {
      self.conditions.push("Username = ?".to_string());
      self.values.push(Value::Text(username.to_string()));
    }
    self
  }

  /// Transfers that failed with exactly `exception`, or with anything when `None`.
  fn exception(mut self, exception: Option<&str>) -> Self {
    if let Some(exception) = exception {
      self.conditions.push("Exception = ?".to_string());
      self.values.push(Value::Text(exception.to_string()));
    }
    self
  }

  /// ` WHERE` and the conditions, or nothing without any.
  fn clause(&self) -> String {
    if self.conditions.is_empty() {
      String::new()
    } else {
      format!(" WHERE {}", self.conditions.join(" AND "))
    }
  }
}

#[cfg(test)]
pub(super) mod tests {
  use api_types::transfers::TransferDirection::{Download, Upload};
  use chrono::{DateTime, TimeZone, Utc};

  use super::*;
  use crate::transfers::fixtures::{Row, insert, transfers};

  /// Every time the databases store.
  pub fn all() -> Window {
    Window {
      start: db::earliest(),
      end: db::latest(),
    }
  }

  /// `hour:minute` on 2026-03-01, UTC.
  pub fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc
      .with_ymd_and_hms(2026, 3, 1, hour, minute, 0)
      .single()
      .expect("a time of day")
  }

  /// A finished transfer of `username` going `direction`, in the state `state`, that ended at
  /// `ended`.
  pub fn row(
    username: &str,
    direction: TransferDirection,
    state: u32,
    ended: DateTime<Utc>,
  ) -> Row {
    let mut row = Row::new(username, direction, state);
    row.requested_at = ended - chrono::Duration::hours(1);
    row.ended_at = Some(ended);
    row
  }

  /// Reports over a day of transfers: two successful uploads on the ends of 10:00 to 12:00 and
  /// one before it, two failed downloads of dave's, one running and one bare completed.
  pub async fn fixture_reports() -> Reports {
    let db = transfers().await;
    let timed = |username: &str, size, speed, ended: DateTime<Utc>| {
      let mut r = row(username, Upload, 48, ended);
      r.size = size;
      r.speed = speed;
      r.started_at = Some(ended - chrono::Duration::seconds(60));
      r.enqueued_at = Some(ended - chrono::Duration::seconds(90));
      r
    };
    let failed = |state, ended, message: &str| {
      let mut r = row("dave", Download, state, ended);
      r.exception = Some(message.to_string());
      r
    };
    insert(
      &db,
      vec![
        timed("alice", 100, 100.0, at(10, 0)),
        timed("bob", 200, 200.0, at(12, 0)),
        timed("carol", 400, 400.0, at(9, 0)),
        failed(272, at(11, 0), "File not shared."),
        failed(80, at(11, 30), "Cancelled"),
        row("erin", Download, 4, at(11, 10)),
        row("frank", Download, 16, at(11, 20)),
      ],
    )
    .await;
    Reports::new(db)
  }
}
