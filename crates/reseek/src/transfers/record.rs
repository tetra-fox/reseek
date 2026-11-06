//! A transfer as its row in `Transfers` holds it, and the record the API shows with the figures
//! derived from it. The engines keep a [`Record`] while they run a transfer and write it whole.

use api_types::transfers::{Transfer, TransferDirection, TransferStates};
use chrono::{DateTime, Utc};
use rusqlite::Row;
use rusqlite::types::Type;
use uuid::Uuid;

use super::{direction_at, text_at};
use crate::db::{self, read_timestamp};

/// The columns [`Record::read`] reads, in its order, for a `SELECT` of `Transfers`.
pub const COLUMNS: &str = "Id, JobId, Username, Direction, Filename, Size, State, RequestedAt, \
                           EnqueuedAt, StartedAt, EndedAt, BytesTransferred, AverageSpeed, \
                           PlaceInQueue, Exception, Attempts, NextAttemptAt, Removed, LocalPath, \
                           Destination, SearchId, Paused";

/// One transfer's row.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
  /// the transfer's id
  pub id: Uuid,
  /// the download job a download was enqueued in: everything queued with it in one go
  pub job_id: Option<Uuid>,
  /// the relative directory under the downloads directory a download's file goes to in place
  /// of the one the download pattern gives
  pub destination: Option<String>,
  /// the search a download's file was found by
  pub search_id: Option<Uuid>,
  /// the peer
  pub username: String,
  /// which way the file moves
  pub direction: TransferDirection,
  /// the remote filename exactly as the peer shares it
  pub filename: String,
  /// the file's size in bytes
  pub size: u64,
  /// where the transfer is
  pub state: TransferStates,
  /// when the transfer was asked for
  pub requested_at: DateTime<Utc>,
  /// when the peer or our queue took it
  pub enqueued_at: Option<DateTime<Utc>>,
  /// when the current attempt started moving bytes
  pub started_at: Option<DateTime<Utc>>,
  /// when it completed
  pub ended_at: Option<DateTime<Utc>>,
  /// bytes moved, counting from offset 0
  pub bytes_transferred: u64,
  /// bytes per second
  pub average_speed: f64,
  /// the place in the peer's queue it last reported
  pub place_in_queue: Option<u32>,
  /// why it failed
  pub exception: Option<String>,
  /// attempts made, for a download
  pub attempts: u32,
  /// when a download waiting to be retried tries again
  pub next_attempt_at: Option<DateTime<Utc>>,
  /// taken out of the listings
  pub removed: bool,
  /// where a download's file was moved to
  pub local_path: Option<String>,
  /// a download the user paused, waiting here until resumed
  pub paused: bool,
}

impl Record {
  /// A new download of `filename` from `username`, asked for now and waiting here.
  pub fn download(username: &str, filename: &str, size: u64) -> Self {
    Self {
      id: Uuid::new_v4(),
      job_id: None,
      destination: None,
      search_id: None,
      username: username.to_string(),
      direction: TransferDirection::Download,
      filename: filename.to_string(),
      size,
      state: TransferStates::Queued | TransferStates::Locally,
      requested_at: Utc::now(),
      enqueued_at: None,
      started_at: None,
      ended_at: None,
      bytes_transferred: 0,
      average_speed: 0.0,
      place_in_queue: None,
      exception: None,
      attempts: 0,
      next_attempt_at: None,
      removed: false,
      local_path: None,
      paused: false,
    }
  }

  /// A new upload of `filename`, `size` bytes, to `username`, asked for and queued now.
  pub fn upload(username: &str, filename: &str, size: u64) -> Self {
    let now = Utc::now();
    Self {
      direction: TransferDirection::Upload,
      requested_at: now,
      enqueued_at: Some(now),
      ..Self::download(username, filename, size)
    }
  }

  /// The record a row of [`COLUMNS`] holds.
  pub fn read(row: &Row) -> rusqlite::Result<Self> {
    let text = |i: usize| row.get::<_, Option<String>>(i);
    let time = |i: usize| -> rusqlite::Result<Option<DateTime<Utc>>> {
      text(i)?.as_deref().map(read_timestamp).transpose()
    };
    let uuid = |i: usize, value: &str| {
      value
        .parse::<Uuid>()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, Type::Text, e.into()))
    };
    let bytes =
      |i: usize| -> rusqlite::Result<u64> { Ok(row.get::<_, i64>(i)?.max(0).unsigned_abs()) };
    Ok(Self {
      id: uuid(0, &row.get::<_, String>(0)?)?,
      job_id: text(1)?.map(|value| uuid(1, &value)).transpose()?,
      destination: text(19)?,
      search_id: text(20)?.map(|value| uuid(20, &value)).transpose()?,
      username: text_at(row, 2)?,
      direction: direction_at(row, 3)?,
      filename: text_at(row, 4)?,
      size: bytes(5)?,
      state: TransferStates::from_bits_retain(row.get::<_, u32>(6)?),
      requested_at: time(7)?.ok_or(rusqlite::Error::InvalidColumnType(
        7,
        "RequestedAt".to_string(),
        Type::Null,
      ))?,
      enqueued_at: time(8)?,
      started_at: time(9)?,
      ended_at: time(10)?,
      bytes_transferred: bytes(11)?,
      average_speed: row.get(12)?,
      place_in_queue: row.get(13)?,
      exception: text(14)?,
      attempts: row.get(15)?,
      next_attempt_at: time(16)?,
      removed: row.get(17)?,
      local_path: text(18)?,
      paused: row.get(21)?,
    })
  }

  /// The record as the API shows it, with what is left, how far along it is and how long it
  /// took; times of a transfer still running are taken against now.
  pub fn transfer(&self) -> Transfer {
    let bytes_remaining = self.size.saturating_sub(self.bytes_transferred);
    let elapsed_time = self.started_at.map(|start| {
      let end = self.ended_at.unwrap_or_else(Utc::now);
      u64::try_from((end - start).num_milliseconds()).unwrap_or(0)
    });
    let percent_complete = if self.size == 0 {
      0.0
    } else {
      (self.bytes_transferred as f64 / self.size as f64 * 100.0).clamp(0.0, 100.0)
    };
    let remaining_time = (self.average_speed > 0.0)
      .then(|| (bytes_remaining as f64 / self.average_speed * 1000.0) as u64);
    Transfer {
      id: self.id,
      job_id: self.job_id,
      username: self.username.clone(),
      direction: self.direction,
      filename: self.filename.clone(),
      size: self.size,
      state: self.state,
      requested_at: self.requested_at,
      enqueued_at: self.enqueued_at,
      started_at: self.started_at,
      ended_at: self.ended_at,
      bytes_transferred: self.bytes_transferred,
      average_speed: self.average_speed,
      place_in_queue: self.place_in_queue,
      exception: self.exception.clone(),
      attempts: self.attempts,
      next_attempt_at: self.next_attempt_at,
      removed: self.removed,
      bytes_remaining,
      elapsed_time,
      percent_complete,
      remaining_time,
      local_path: self.local_path.clone(),
      paused: self.paused,
      state_history: None,
    }
  }

  /// Takes the times, the bytes moved and the speed the client's `snapshot` of the transfer
  /// has.
  pub fn take_figures(&mut self, snapshot: &soulseek_client::Transfer) {
    let time = |at: Option<std::time::SystemTime>| at.map(DateTime::<Utc>::from);
    self.started_at = time(snapshot.start_time);
    self.ended_at = time(snapshot.end_time);
    self.bytes_transferred = snapshot.bytes_transferred;
    self.average_speed = snapshot.average_speed;
  }

  /// Whether the transfer has finished, whatever the outcome.
  pub fn is_completed(&self) -> bool {
    self.state.contains(TransferStates::Completed)
  }
}

/// The record a row of [`COLUMNS`] holds, as the API shows it.
pub fn transfer(row: &Row) -> rusqlite::Result<Transfer> {
  Record::read(row).map(|record| record.transfer())
}

/// The text a nullable time column is written with.
pub(super) fn time_text(time: Option<DateTime<Utc>>) -> Option<String> {
  time.map(db::timestamp)
}

#[cfg(test)]
mod tests {
  use super::super::fixtures::{Row as Fixture, insert, transfers};
  use super::*;

  #[tokio::test]
  async fn a_record_reads_back_with_its_derived_figures() {
    let db = transfers().await;
    let started: DateTime<Utc> = "2026-05-06T10:00:00Z".parse().unwrap();
    let mut row = Fixture::new("alice", TransferDirection::Download, 16 | 32);
    row.size = 1000;
    row.bytes = 1200;
    row.speed = 0.0;
    row.started_at = Some(started);
    row.ended_at = Some(started + chrono::Duration::milliseconds(32_800));
    let id = row.id;
    insert(&db, vec![row]).await;
    let read = db
      .read(move |conn| {
        conn.query_row(
          &format!("SELECT {COLUMNS} FROM Transfers WHERE Id = ?1"),
          [id.to_string()],
          transfer,
        )
      })
      .await
      .unwrap();
    assert_eq!(read.id, id);
    assert_eq!(
      read.state,
      TransferStates::Completed | TransferStates::Succeeded
    );
    assert_eq!(read.elapsed_time, Some(32_800));
    assert_eq!(read.bytes_remaining, 0, "never below zero");
    assert_eq!(read.percent_complete, 100.0, "never above a hundred");
    assert_eq!(read.remaining_time, None, "no speed, no estimate");
    assert_eq!(read.local_path, None);
  }
}
