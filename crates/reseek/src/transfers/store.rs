//! Writing transfer records: a new record, a record written whole, what a running transfer
//! changes, its progress, and the states each transfer entered, of which the newest
//! [`HISTORY_KEPT`] are kept. A record is written in its state only over a row in a state the
//! state machine allows it to follow, so a write that races another, such as a cancel that
//! raced the download completing, never overwrites a completed row.

use api_types::transfers::{TransferStateChange, TransferStates};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use super::record::time_text;
use super::state::{Refused, column, entered_from, in_states, state_at};
use super::{COLUMNS, Record, direction_text};
use crate::db::{read_timestamp, timestamp};

/// How many states each transfer's history keeps; the oldest beyond them are dropped.
pub const HISTORY_KEPT: u32 = 64;

/// Writes a new record, and the state it starts in to its history.
pub fn insert(conn: &Connection, record: &Record) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO Transfers (Id, JobId, Username, Direction, Filename, Size, State, \
     StateDescription, RequestedAt, EnqueuedAt, StartedAt, EndedAt, BytesTransferred, \
     AverageSpeed, PlaceInQueue, Exception, Removed, Attempts, NextAttemptAt, LocalPath, \
     Destination, SearchId, Paused, QueuePosition) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, \
     ?19, ?20, ?21, ?22, ?23, ?24)",
    params![
      record.id.to_string(),
      record.job_id.map(|id| id.to_string()),
      record.username,
      direction_text(record.direction),
      record.filename,
      i64::try_from(record.size).unwrap_or(i64::MAX),
      column(record.state.kind()),
      record.state.to_string(),
      timestamp(record.requested_at),
      time_text(record.enqueued_at),
      time_text(record.started_at),
      time_text(record.ended_at),
      i64::try_from(record.bytes_transferred).unwrap_or(i64::MAX),
      record.average_speed,
      record.place_in_queue(),
      record.exception,
      record.removed,
      record.attempts,
      time_text(record.next_attempt_at()),
      record.local_path,
      record.destination,
      record.search_id.map(|id| id.to_string()),
      record.paused,
      record.queue_position,
    ],
  )?;
  entered(conn, record.id, record.state.kind(), record.requested_at)
}

/// How a write of a record in its state went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
  /// it was written
  Written,
  /// there is no row of it
  Gone,
  /// its row is in a state the record's may not follow, and was left as it is
  Refused(Refused),
}

/// The condition that holds for a row `record` may be written over: one in a state its state
/// may follow.
fn followed_by(record: &Record) -> String {
  in_states(entered_from(record.state.kind()))
}

/// How a write of `record` that changed `changed` rows went, telling a refused write from a
/// row that is gone by the state the row is in.
fn saved(conn: &Connection, record: &Record, changed: usize) -> rusqlite::Result<Saved> {
  if changed > 0 {
    return Ok(Saved::Written);
  }
  let state = conn
    .query_row(
      "SELECT State FROM Transfers WHERE Id = ?1",
      [record.id.to_string()],
      |row| state_at(row, 0),
    )
    .optional()?;
  Ok(match state {
    Some(from) => Saved::Refused(Refused {
      from,
      to: record.state.kind(),
    }),
    None => Saved::Gone,
  })
}

/// The place in the local queue after every download's, for the next one enqueued.
pub fn next_position(conn: &Connection) -> rusqlite::Result<f64> {
  conn.query_row(
    "SELECT coalesce(max(QueuePosition), 0) + 1 FROM Transfers",
    [],
    |row| row.get(0),
  )
}

/// Writes where each of `placed` downloads stands in the local queue.
pub fn place(conn: &Connection, placed: &[(Uuid, f64)]) -> rusqlite::Result<()> {
  let mut statement =
    conn.prepare_cached("UPDATE Transfers SET QueuePosition = ?2 WHERE Id = ?1")?;
  for (id, position) in placed {
    statement.execute(params![id.to_string(), position])?;
  }
  Ok(())
}

/// Writes everything about `record` that changes while it runs, over a row in a state its state
/// may follow. A record written as removed keeps the time it was first removed.
pub fn save(conn: &Connection, record: &Record) -> rusqlite::Result<Saved> {
  let changed = conn.execute(
    &format!(
      "UPDATE Transfers SET Size = ?2, State = ?3, StateDescription = ?4, EnqueuedAt = ?5, \
       StartedAt = ?6, EndedAt = ?7, BytesTransferred = ?8, AverageSpeed = ?9, \
       PlaceInQueue = ?10, Exception = ?11, Attempts = ?12, NextAttemptAt = ?13, Removed = ?14, \
       RemovedAt = CASE WHEN ?14 THEN coalesce(RemovedAt, ?15) END, LocalPath = ?16, \
       Paused = ?17 WHERE Id = ?1 AND {}",
      followed_by(record)
    ),
    params![
      record.id.to_string(),
      i64::try_from(record.size).unwrap_or(i64::MAX),
      column(record.state.kind()),
      record.state.to_string(),
      time_text(record.enqueued_at),
      time_text(record.started_at),
      time_text(record.ended_at),
      i64::try_from(record.bytes_transferred).unwrap_or(i64::MAX),
      record.average_speed,
      record.place_in_queue(),
      record.exception,
      record.attempts,
      time_text(record.next_attempt_at()),
      record.removed,
      timestamp(Utc::now()),
      record.local_path,
      record.paused,
    ],
  )?;
  saved(conn, record, changed)
}

/// Writes what a running transfer changes: its state, times, bytes, speed and error, leaving
/// the rest, whether it was removed included, as the row has it, over a row in a state its
/// state may follow.
pub fn save_state(conn: &Connection, record: &Record) -> rusqlite::Result<Saved> {
  let changed = conn.execute(
    &format!(
      "UPDATE Transfers SET State = ?2, StateDescription = ?3, StartedAt = ?4, EndedAt = ?5, \
       BytesTransferred = ?6, AverageSpeed = ?7, Exception = ?8 WHERE Id = ?1 AND {}",
      followed_by(record)
    ),
    params![
      record.id.to_string(),
      column(record.state.kind()),
      record.state.to_string(),
      time_text(record.started_at),
      time_text(record.ended_at),
      i64::try_from(record.bytes_transferred).unwrap_or(i64::MAX),
      record.average_speed,
      record.exception,
    ],
  )?;
  saved(conn, record, changed)
}

/// Ends the transfer `id` in the completed `state` at `at` with `exception`, unless it ended
/// already; the retry time and the queue place go with the wait they belonged to. Returns
/// whether it was ended here.
pub fn end_unless_ended(
  conn: &Connection,
  id: Uuid,
  state: TransferStates,
  at: DateTime<Utc>,
  exception: &str,
) -> rusqlite::Result<bool> {
  let changed = conn.execute(
    &format!(
      "UPDATE Transfers SET State = ?2, StateDescription = ?3, EndedAt = ?4, Exception = ?5, \
       NextAttemptAt = NULL, PlaceInQueue = NULL WHERE Id = ?1 AND {}",
      in_states(entered_from(state))
    ),
    params![
      id.to_string(),
      column(state),
      state.as_str(),
      time_text(Some(at)),
      exception,
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
      column(TransferStates::InProgress),
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
    params![id, column(state), timestamp(at)],
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
        state: state_at(row, 0)?,
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
  use api_types::transfers::TransferOutcome;

  use super::*;
  use crate::transfers::fixtures::transfers;
  use crate::transfers::state::State;

  const SUCCEEDED: State = State::Completed(TransferOutcome::Succeeded);
  const CANCELED: State = State::Completed(TransferOutcome::Cancelled);

  #[tokio::test]
  async fn a_record_is_written_whole_and_reads_back_the_same() {
    let db = transfers().await;
    let mut record = Record::download("alice", "@@a\\b.flac", 10);
    record.requested_at = "2026-05-06T10:00:00.123456Z".parse().unwrap();
    let written = record.clone();
    db.write(move |conn| insert(conn, &written)).await.unwrap();
    record.enter(SUCCEEDED).unwrap();
    record.bytes_transferred = 10;
    record.ended_at = Some(record.requested_at);
    record.local_path = Some("/music/b.flac".to_string());
    record.removed = true;
    let saved = record.clone();
    let written = db.write(move |conn| save(conn, &saved)).await.unwrap();
    assert_eq!(written, Saved::Written);
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
  async fn every_state_is_written_and_read_back_with_its_data() {
    let db = transfers().await;
    let at: DateTime<Utc> = "2026-05-06T10:00:00.123456Z".parse().unwrap();
    for kind in TransferStates::ALL {
      let mut record = Record::download("alice", kind.as_str(), 10);
      record.requested_at = at;
      record.state = match State::of(kind) {
        State::QueuedLocally { .. } => State::QueuedLocally {
          next_attempt_at: Some(at),
        },
        State::QueuedRemotely { .. } => State::QueuedRemotely {
          place_in_queue: Some(7),
        },
        other => other,
      };
      let written = record.clone();
      db.write(move |conn| insert(conn, &written)).await.unwrap();
      let id = record.id;
      let read = db.read(move |conn| get(conn, id)).await.unwrap().unwrap();
      assert_eq!(read, record, "{kind}");
      let shown = serde_json::to_value(read.transfer()).unwrap();
      assert_eq!(shown["state"], kind.as_str());
      let history = db.read(move |conn| history(conn, id)).await.unwrap();
      assert_eq!(history[0].state, kind);
    }
  }

  #[tokio::test]
  async fn a_state_written_leaves_a_removal_made_meanwhile() {
    let db = transfers().await;
    let mut record = Record::download("alice", "a.flac", 10);
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
    record.enter(CANCELED).unwrap();
    let saved = record.clone();
    let written = db
      .write(move |conn| save_state(conn, &saved))
      .await
      .unwrap();
    assert_eq!(written, Saved::Written);
    let read = db.read(move |conn| get(conn, id)).await.unwrap().unwrap();
    assert!(read.removed);
    assert_eq!(read.state, record.state);
  }

  #[tokio::test]
  async fn progress_never_touches_a_transfer_that_is_not_in_progress() {
    let db = transfers().await;
    let mut record = Record::download("alice", "a.flac", 10);
    record.state = State::InProgress;
    let written = record.clone();
    db.write(move |conn| insert(conn, &written)).await.unwrap();
    let id = record.id;
    assert!(
      db.write(move |conn| progress(conn, id, 5, 1.0))
        .await
        .unwrap()
    );
    record.enter(SUCCEEDED).unwrap();
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

  /// A cancel or a pause reads a download that is still running, the download completes, and
  /// then the cancel or the pause writes its stale copy: the write is refused and the download
  /// stays as it completed.
  #[tokio::test]
  async fn a_stale_write_never_overwrites_a_download_that_completed_meanwhile() {
    let db = transfers().await;
    let at: DateTime<Utc> = "2026-05-06T10:00:00.123456Z".parse().unwrap();
    let mut running = Record::download("alice", "b.flac", 3);
    running.requested_at = at;
    running.state = State::InProgress;
    let written = running.clone();
    db.write(move |conn| insert(conn, &written)).await.unwrap();
    let id = running.id;
    let stale = running.clone();
    running.ended_at = Some(at);
    running.enter(SUCCEEDED).unwrap();
    running.local_path = Some("/music/b.flac".into());
    let done = running.clone();
    db.write(move |conn| save(conn, &done)).await.unwrap();

    let mut canceled = stale.clone();
    canceled
      .complete(TransferOutcome::Cancelled, Some("canceled".into()))
      .unwrap();
    let mut paused = stale;
    paused.enter(State::WAITING).unwrap();
    paused.paused = true;
    for write in [canceled, paused] {
      let to = write.state.kind();
      let saved = db.write(move |conn| save(conn, &write)).await.unwrap();
      assert_eq!(
        saved,
        Saved::Refused(Refused {
          from: TransferStates::Completed(TransferOutcome::Succeeded),
          to,
        })
      );
    }
    let now = Utc::now();
    let cancel = TransferStates::Completed(TransferOutcome::Cancelled);
    let ended = db
      .write(move |conn| end_unless_ended(conn, id, cancel, now, "canceled"))
      .await
      .unwrap();
    assert!(!ended);
    let read = db.read(move |conn| get(conn, id)).await.unwrap().unwrap();
    assert_eq!(read, running);
  }

  #[tokio::test]
  async fn the_history_keeps_the_newest_sixty_four_states() {
    let db = transfers().await;
    let record = Record::download("alice", "a.flac", 10);
    let id = record.id;
    db.write(move |conn| {
      insert(conn, &record)?;
      for n in 0..70 {
        let state = if n % 2 == 0 {
          TransferStates::InProgress
        } else {
          TransferStates::QueuedRemotely
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
      TransferStates::QueuedRemotely
    );
  }
}
