//! Writing transfer records: a new record, a record written whole, what a running transfer
//! changes, its progress, and the states each transfer entered, of which the newest
//! [`HISTORY_KEPT`] are kept.

use api_types::transfers::{TransferStateChange, TransferStates};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use super::record::time_text;
use super::{COLUMNS, Record, direction_text};
use crate::db::{read_timestamp, timestamp};

/// How many states each transfer's history keeps; the oldest beyond them are dropped.
pub const HISTORY_KEPT: u32 = 64;

/// Writes a new record, and the state it starts in to its history.
pub fn insert(conn: &Connection, record: &Record) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO Transfers (Id, BatchId, Username, Direction, Filename, Size, State, \
     StateDescription, RequestedAt, EnqueuedAt, StartedAt, EndedAt, BytesTransferred, \
     AverageSpeed, PlaceInQueue, Exception, Removed, Attempts, NextAttemptAt, LocalPath, JobId) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, \
     ?19, ?20, ?21)",
    params![
      record.id.to_string(),
      record.batch_id.map(|id| id.to_string()),
      record.username,
      direction_text(record.direction),
      record.filename,
      i64::try_from(record.size).unwrap_or(i64::MAX),
      record.state.bits(),
      record.state.to_string(),
      timestamp(record.requested_at),
      time_text(record.enqueued_at),
      time_text(record.started_at),
      time_text(record.ended_at),
      i64::try_from(record.bytes_transferred).unwrap_or(i64::MAX),
      record.average_speed,
      record.place_in_queue,
      record.exception,
      record.removed,
      record.attempts,
      time_text(record.next_attempt_at),
      record.local_path,
      record.job_id.map(|id| id.to_string()),
    ],
  )?;
  entered(conn, record.id, record.state, record.requested_at)
}

/// Writes everything about `record` that changes while it runs; returns whether its row was
/// there. A record written as removed keeps the time it was first removed.
pub fn save(conn: &Connection, record: &Record) -> rusqlite::Result<bool> {
  let changed = conn.execute(
    "UPDATE Transfers SET Size = ?2, State = ?3, StateDescription = ?4, EnqueuedAt = ?5, \
     StartedAt = ?6, EndedAt = ?7, BytesTransferred = ?8, AverageSpeed = ?9, \
     PlaceInQueue = ?10, Exception = ?11, Attempts = ?12, NextAttemptAt = ?13, Removed = ?14, \
     RemovedAt = CASE WHEN ?14 THEN coalesce(RemovedAt, ?15) END, LocalPath = ?16 \
     WHERE Id = ?1",
    params![
      record.id.to_string(),
      i64::try_from(record.size).unwrap_or(i64::MAX),
      record.state.bits(),
      record.state.to_string(),
      time_text(record.enqueued_at),
      time_text(record.started_at),
      time_text(record.ended_at),
      i64::try_from(record.bytes_transferred).unwrap_or(i64::MAX),
      record.average_speed,
      record.place_in_queue,
      record.exception,
      record.attempts,
      time_text(record.next_attempt_at),
      record.removed,
      timestamp(Utc::now()),
      record.local_path,
    ],
  )?;
  Ok(changed > 0)
}

/// Writes what a running transfer changes: its state, times, bytes, speed and error, leaving
/// the rest, whether it was removed included, as the row has it. Returns whether its row was
/// there.
pub fn save_state(conn: &Connection, record: &Record) -> rusqlite::Result<bool> {
  let changed = conn.execute(
    "UPDATE Transfers SET State = ?2, StateDescription = ?3, StartedAt = ?4, EndedAt = ?5, \
     BytesTransferred = ?6, AverageSpeed = ?7, Exception = ?8 WHERE Id = ?1",
    params![
      record.id.to_string(),
      record.state.bits(),
      record.state.to_string(),
      time_text(record.started_at),
      time_text(record.ended_at),
      i64::try_from(record.bytes_transferred).unwrap_or(i64::MAX),
      record.average_speed,
      record.exception,
    ],
  )?;
  Ok(changed > 0)
}

/// Ends the transfer `id` in `state` at `at` with `exception`, unless it ended already; the
/// retry time and the queue place go with the wait they belonged to. Returns whether it was
/// ended here.
pub fn end_unless_ended(
  conn: &Connection,
  id: Uuid,
  state: TransferStates,
  at: DateTime<Utc>,
  exception: &str,
) -> rusqlite::Result<bool> {
  let changed = conn.execute(
    "UPDATE Transfers SET State = ?2, StateDescription = ?3, EndedAt = ?4, Exception = ?5, \
     NextAttemptAt = NULL, PlaceInQueue = NULL WHERE Id = ?1 AND (State & ?6) = 0",
    params![
      id.to_string(),
      state.bits(),
      state.to_string(),
      time_text(Some(at)),
      exception,
      TransferStates::Completed.bits(),
    ],
  )?;
  Ok(changed > 0)
}

/// Records the bytes moved and the speed of a transfer in progress; a transfer in any other
/// state is left alone, so a late sample never touches a finished one. Returns whether it was
/// written.
pub fn progress(conn: &Connection, id: Uuid, bytes: u64, speed: f64) -> rusqlite::Result<bool> {
  let changed = conn.execute(
    "UPDATE Transfers SET BytesTransferred = ?2, AverageSpeed = ?3 WHERE Id = ?1 AND State = ?4",
    params![
      id.to_string(),
      i64::try_from(bytes).unwrap_or(i64::MAX),
      speed,
      TransferStates::InProgress.bits(),
    ],
  )?;
  Ok(changed > 0)
}

/// Adds `state`, entered at `at`, to the history of the transfer `id`, dropping the oldest
/// beyond [`HISTORY_KEPT`].
pub fn entered(
  conn: &Connection,
  id: Uuid,
  state: TransferStates,
  at: DateTime<Utc>,
) -> rusqlite::Result<()> {
  let id = id.to_string();
  conn.execute(
    "INSERT INTO transfers_state_history (transfer_id, state, at) VALUES (?1, ?2, ?3)",
    params![id, state.bits(), timestamp(at)],
  )?;
  conn.execute(
    "DELETE FROM transfers_state_history WHERE transfer_id = ?1 AND rowid NOT IN \
     (SELECT rowid FROM transfers_state_history WHERE transfer_id = ?1 \
      ORDER BY rowid DESC LIMIT ?2)",
    params![id, HISTORY_KEPT],
  )?;
  Ok(())
}

/// The states the transfer `id` entered, oldest first.
pub fn history(conn: &Connection, id: Uuid) -> rusqlite::Result<Vec<TransferStateChange>> {
  let mut statement = conn.prepare_cached(
    "SELECT state, at FROM transfers_state_history WHERE transfer_id = ?1 ORDER BY rowid",
  )?;
  statement
    .query_map([id.to_string()], |row| {
      Ok(TransferStateChange {
        state: TransferStates::from_bits_retain(row.get(0)?),
        at: read_timestamp(&row.get::<_, String>(1)?)?,
      })
    })?
    .collect()
}

/// The record `id`, whatever its direction.
pub fn get(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<Record>> {
  conn
    .query_row(
      &format!("SELECT {COLUMNS} FROM Transfers WHERE Id = ?1"),
      [id.to_string()],
      Record::read,
    )
    .optional()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::fixtures::transfers;

  #[tokio::test]
  async fn a_record_is_written_whole_and_reads_back_the_same() {
    let db = transfers().await;
    let mut record = Record::download("alice", "@@a\\b.flac", 10, None);
    record.requested_at = "2026-05-06T10:00:00.123456Z".parse().unwrap();
    let written = record.clone();
    db.write(move |conn| insert(conn, &written)).await.unwrap();
    record.state = TransferStates::Completed | TransferStates::Succeeded;
    record.bytes_transferred = 10;
    record.ended_at = Some(record.requested_at);
    record.local_path = Some("/music/b.flac".to_string());
    record.removed = true;
    let saved = record.clone();
    assert!(db.write(move |conn| save(conn, &saved)).await.unwrap());
    let id = record.id;
    let read = db.read(move |conn| get(conn, id)).await.unwrap().unwrap();
    assert_eq!(read, record);
    let removed_at: Option<String> = db
      .read(move |conn| {
        conn.query_row(
          "SELECT RemovedAt FROM Transfers WHERE Id = ?1",
          [id.to_string()],
          |row| row.get(0),
        )
      })
      .await
      .unwrap();
    assert!(removed_at.is_some(), "removing it stamps when");
  }

  #[tokio::test]
  async fn a_state_written_leaves_a_removal_made_meanwhile() {
    let db = transfers().await;
    let mut record = Record::download("alice", "a.flac", 10, None);
    let written = record.clone();
    db.write(move |conn| insert(conn, &written)).await.unwrap();
    let id = record.id;
    db.write(move |conn| {
      conn.execute(
        "UPDATE Transfers SET Removed = 1 WHERE Id = ?1",
        [id.to_string()],
      )
    })
    .await
    .unwrap();
    record.state = TransferStates::Completed | TransferStates::Cancelled;
    record.ended_at = Some(Utc::now());
    let saved = record.clone();
    assert!(
      db.write(move |conn| save_state(conn, &saved))
        .await
        .unwrap()
    );
    let read = db.read(move |conn| get(conn, id)).await.unwrap().unwrap();
    assert!(read.removed);
    assert_eq!(read.state, record.state);
  }

  #[tokio::test]
  async fn progress_never_touches_a_transfer_that_is_not_in_progress() {
    let db = transfers().await;
    let mut record = Record::download("alice", "a.flac", 10, None);
    record.state = TransferStates::InProgress;
    let written = record.clone();
    db.write(move |conn| insert(conn, &written)).await.unwrap();
    let id = record.id;
    assert!(
      db.write(move |conn| progress(conn, id, 5, 1.0))
        .await
        .unwrap()
    );
    record.state = TransferStates::Completed | TransferStates::Succeeded;
    record.bytes_transferred = 10;
    let saved = record.clone();
    db.write(move |conn| save(conn, &saved)).await.unwrap();
    assert!(
      !db
        .write(move |conn| progress(conn, id, 7, 1.0))
        .await
        .unwrap()
    );
    let read = db.read(move |conn| get(conn, id)).await.unwrap().unwrap();
    assert_eq!(read.bytes_transferred, 10);
  }

  #[tokio::test]
  async fn the_history_keeps_the_newest_sixty_four_states() {
    let db = transfers().await;
    let record = Record::download("alice", "a.flac", 10, None);
    let id = record.id;
    db.write(move |conn| {
      insert(conn, &record)?;
      for n in 0..70 {
        let state = if n % 2 == 0 {
          TransferStates::InProgress
        } else {
          TransferStates::Queued | TransferStates::Remotely
        };
        entered(conn, id, state, Utc::now())?;
      }
      Ok(())
    })
    .await
    .unwrap();
    let history = db.read(move |conn| history(conn, id)).await.unwrap();
    assert_eq!(history.len(), HISTORY_KEPT as usize);
    // the insert's state and the first six entered are gone
    assert_eq!(history[0].state, TransferStates::InProgress);
    assert_eq!(
      history.last().unwrap().state,
      TransferStates::Queued | TransferStates::Remotely
    );
  }
}
