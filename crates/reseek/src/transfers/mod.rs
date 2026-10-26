//! Transfers: the database every download and upload is recorded in, and reading a record back
//! as the API shows it.

pub mod categories;
#[cfg(test)]
pub mod fixtures;

use api_types::transfers::{Transfer, TransferDirection, TransferStates};
use chrono::{DateTime, Utc};
use rusqlite::Row;
use rusqlite::types::Type;

use crate::db::{self, Database, DbError, Location, Schema};

/// `transfers.db`: the `Transfers` and `Batches` tables.
pub static SCHEMA: Schema = db::schema!("transfers", ["0001_initial"]);

/// Opens `transfers.db` in `location`.
pub async fn open(location: &Location) -> Result<Database, DbError> {
  Database::open(&SCHEMA, location).await
}

/// The columns [`transfer`] reads, in its order, for a `SELECT` of `Transfers`.
pub const COLUMNS: &str = "Id, BatchId, Username, Direction, Filename, Size, State, RequestedAt, \
                           EnqueuedAt, StartedAt, EndedAt, BytesTransferred, AverageSpeed, \
                           PlaceInQueue, Exception, Attempts, NextAttemptAt, Removed";

/// A direction as the database stores it.
pub fn direction_text(direction: TransferDirection) -> &'static str {
  match direction {
    TransferDirection::Download => "Download",
    TransferDirection::Upload => "Upload",
  }
}

/// The record a row of [`COLUMNS`] holds, with the figures derived from it: what is left, how
/// far along it is and how long it took. Times derived while it runs are taken against now.
pub fn transfer(row: &Row) -> rusqlite::Result<Transfer> {
  let text = |i: usize| row.get::<_, Option<String>>(i);
  let time = |i: usize| -> rusqlite::Result<Option<DateTime<Utc>>> {
    text(i)?.as_deref().map(db::read_timestamp).transpose()
  };
  let uuid = |i: usize, value: &str| {
    value
      .parse::<uuid::Uuid>()
      .map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, Type::Text, e.into()))
  };
  let id = uuid(0, &row.get::<_, String>(0)?)?;
  let batch_id = text(1)?.map(|value| uuid(1, &value)).transpose()?;
  let direction = match row.get::<_, String>(3)?.as_str() {
    "Download" => TransferDirection::Download,
    "Upload" => TransferDirection::Upload,
    other => {
      return Err(rusqlite::Error::FromSqlConversionFailure(
        3,
        Type::Text,
        format!("{other} is no direction").into(),
      ));
    }
  };
  let size = row.get::<_, i64>(5)?.max(0).unsigned_abs();
  let state = TransferStates::from_bits_retain(row.get::<_, u32>(6)?);
  let requested_at = time(7)?.ok_or(rusqlite::Error::InvalidColumnType(
    7,
    "RequestedAt".to_string(),
    Type::Null,
  ))?;
  let started_at = time(9)?;
  let ended_at = time(10)?;
  let bytes_transferred = row.get::<_, i64>(11)?.max(0).unsigned_abs();
  let average_speed: f64 = row.get(12)?;
  let bytes_remaining = size.saturating_sub(bytes_transferred);
  let elapsed_time = started_at.map(|start| {
    let end = ended_at.unwrap_or_else(Utc::now);
    u64::try_from((end - start).num_milliseconds()).unwrap_or(0)
  });
  let percent_complete = if size == 0 {
    0.0
  } else {
    (bytes_transferred as f64 / size as f64 * 100.0).clamp(0.0, 100.0)
  };
  let remaining_time =
    (average_speed > 0.0).then(|| (bytes_remaining as f64 / average_speed * 1000.0) as u64);
  Ok(Transfer {
    id,
    batch_id,
    username: text(2)?.unwrap_or_default(),
    direction,
    filename: text(4)?.unwrap_or_default(),
    size,
    state,
    requested_at,
    enqueued_at: time(8)?,
    started_at,
    ended_at,
    bytes_transferred,
    average_speed,
    place_in_queue: row.get(13)?,
    exception: text(14)?,
    attempts: row.get(15)?,
    next_attempt_at: time(16)?,
    removed: row.get(17)?,
    bytes_remaining,
    elapsed_time,
    percent_complete,
    remaining_time,
    local_path: None,
    state_history: None,
  })
}

#[cfg(test)]
mod tests {
  use super::fixtures::{Row as Fixture, insert, transfers};
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
  }
}
