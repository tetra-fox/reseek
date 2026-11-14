//! Transfers: the database every download and upload is recorded in, reading a record back as
//! the API shows it, and what both directions share: writing records, the listings, removal and
//! retention. The engines are [`downloads`] and [`uploads`]; [`service::Transfers`] answers
//! the API with both.

pub mod categories;
pub mod cleanup;
pub mod downloads;
#[cfg(test)]
pub mod fixtures;
pub mod listing;
pub mod prune;
pub mod record;
pub mod removal;
pub mod service;
pub mod speed;
pub mod state;
pub mod store;
pub mod uploads;

use std::time::Duration;

use api_types::transfers::TransferDirection;
use rusqlite::Row;
use rusqlite::types::Type;
use soulseek_client::LogLimiter;
use tracing::warn;

use crate::db::{self, Database, DbError, Location, Schema};
pub use record::{ASKED_ORDER, COLUMNS, NEWEST_FIRST, Record, transfer};

/// `transfers.db`: the `Transfers` table, each transfer's state history, and which speed limits
/// are in force.
pub static SCHEMA: Schema = db::schema!(
  "transfers",
  [
    "0001_initial",
    "0002_queue_position",
    "0003_folder",
    "0004_state_lost"
  ]
);

/// How long the tasks of running transfers get to end at shutdown; what they leave unfinished
/// is ended after.
const STOPPING: Duration = Duration::from_secs(5);

/// Why a transfer canceled because its user was blocked ended.
const BLOCKED: &str = "Blocked";

/// More states than one try of a transfer reports.
const STATES: usize = 16;

/// How often a transfer's progress is written and pushed at most.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Opens `transfers.db` in `location`.
pub async fn open(location: &Location) -> Result<Database, DbError> {
  Database::open(&SCHEMA, location).await
}

/// A direction as the database stores it.
pub fn direction_text(direction: TransferDirection) -> &'static str {
  match direction {
    TransferDirection::Download => "Download",
    TransferDirection::Upload => "Upload",
  }
}

/// The text in the nullable column `i` of `row`, such as `Username` or `Filename`, empty for
/// NULL: the transfer engines always write them, so a NULL is a row from elsewhere and reads
/// as no name.
pub fn text_at(row: &Row, i: usize) -> rusqlite::Result<String> {
  Ok(row.get::<_, Option<String>>(i)?.unwrap_or_default())
}

/// Rows left out of reads for a value that cannot be read, which every listing repeats.
static UNREADABLE: LogLimiter = LogLimiter::new();

/// What `read` makes of `row`, or none for a row holding a value no build of the daemon writes,
/// such as a state it does not know, so one such row does not fail a read of many. The row is
/// logged by its id and peer, which a cancel with removal of it takes out of the listings.
pub fn readable<T>(
  row: &Row,
  read: impl FnOnce(&Row) -> rusqlite::Result<T>,
) -> rusqlite::Result<Option<T>> {
  match read(row) {
    Ok(value) => Ok(Some(value)),
    Err(e) if unreadable(&e) => {
      if let Some(suppressed) = UNREADABLE.allow() {
        // the id and peer are named when the read selected them
        let id = row.get::<_, String>("Id").ok();
        let username = row.get::<_, Option<String>>("Username").ok().flatten();
        warn!(?id, ?username, error = %e, suppressed, "transfer that cannot be read left out");
      }
      Ok(None)
    }
    Err(e) => Err(e),
  }
}

/// Whether `error` is a value of a row that cannot be read as a transfer, rather than a
/// failure of the database.
pub fn unreadable(error: &rusqlite::Error) -> bool {
  matches!(
    error,
    rusqlite::Error::FromSqlConversionFailure(..)
      | rusqlite::Error::InvalidColumnType(..)
      | rusqlite::Error::IntegralValueOutOfRange(..)
  )
}

/// The direction in the column `i` of `row`, as [`direction_text`] wrote it.
pub fn direction_at(row: &Row, i: usize) -> rusqlite::Result<TransferDirection> {
  match row.get_ref(i)?.as_str()? {
    "Download" => Ok(TransferDirection::Download),
    "Upload" => Ok(TransferDirection::Upload),
    other => Err(rusqlite::Error::FromSqlConversionFailure(
      i,
      Type::Text,
      format!("{other} is no direction").into(),
    )),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::fixtures::{Row, insert};
  use api_types::transfers::TransferStates;

  /// `transfers.db` before the local queue's order was kept.
  static BEFORE_QUEUE_POSITIONS: Schema = db::schema!("transfers", ["0001_initial"]);

  #[tokio::test]
  async fn downloads_written_before_places_were_kept_are_placed_in_the_order_they_were_asked() {
    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    let old = Database::open(&BEFORE_QUEUE_POSITIONS, &location)
      .await
      .unwrap();
    let at = |minute: u32| format!("2026-11-01T10:0{minute}:00Z").parse().unwrap();
    let later = Row {
      requested_at: at(2),
      ..Row::new(
        "alice",
        TransferDirection::Download,
        TransferStates::QueuedRemotely,
      )
    };
    let earlier = Row {
      requested_at: at(1),
      ..Row::new(
        "alice",
        TransferDirection::Download,
        TransferStates::QueuedRemotely,
      )
    };
    let upload = Row::new(
      "bob",
      TransferDirection::Upload,
      TransferStates::QueuedLocally,
    );
    let ids = [later.id, earlier.id, upload.id];
    insert(&old, vec![later, earlier, upload]).await;
    drop(old);
    let migrated = open(&location).await.unwrap();
    let (placed, next) = migrated
      .read(move |conn| {
        let placed = ids
          .iter()
          .map(|id| store::get(conn, *id).map(|record| record.unwrap().queue_position))
          .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((placed, store::next_position(conn)?))
      })
      .await
      .unwrap();
    assert_eq!(placed, [Some(2.0), Some(1.0), None]);
    assert_eq!(next, 3.0);
  }

  /// `transfers.db` before rows whose state was lost were ended.
  static BEFORE_STATE_LOST: Schema = db::schema!(
    "transfers",
    ["0001_initial", "0002_queue_position", "0003_folder"]
  );

  #[tokio::test]
  async fn rows_whose_state_was_lost_end_errored_at_the_last_time_they_were_known() {
    use TransferStates::{Completed, InProgress, QueuedLocally};
    use api_types::transfers::TransferOutcome::{Errored, Succeeded};
    use chrono::{DateTime, Utc};

    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    let old = Database::open(&BEFORE_STATE_LOST, &location).await.unwrap();
    let at =
      |minute: u32| -> DateTime<Utc> { format!("2026-11-01T10:{minute:02}:00Z").parse().unwrap() };
    let row = |name: &str, state| Row {
      requested_at: at(0),
      ..Row::new(name, TransferDirection::Download, state)
    };
    let none = Row {
      started_at: Some(at(5)),
      ..row("none", QueuedLocally)
    };
    let queued = Row {
      enqueued_at: Some(at(3)),
      ..row("queued", QueuedLocally)
    };
    let completed = Row {
      ended_at: Some(at(9)),
      exception: Some("Connection refused".to_string()),
      ..row("completed", QueuedLocally)
    };
    let unknown = row("unknown", QueuedLocally);
    let no_end = Row {
      started_at: Some(at(4)),
      ..row("no end", Completed(Succeeded))
    };
    let blank = row("blank", QueuedLocally);
    let running = Row {
      started_at: Some(at(1)),
      ..row("running", InProgress)
    };
    let done = Row {
      ended_at: Some(at(8)),
      ..row("done", Completed(Succeeded))
    };
    // each with the time it ends at: its end when it has one, else the latest time it holds
    let lost = [
      (none.id, at(5)),
      (queued.id, at(7)),
      (completed.id, at(9)),
      (unknown.id, at(0)),
      (no_end.id, at(4)),
    ];
    let kept = [(running.id, running.state), (done.id, done.state)];
    let blank_id = blank.id;
    let raw = [
      (none.id, 0),
      (queued.id, 2),
      (completed.id, 16),
      (unknown.id, 2 | 2048 | 4096),
      (blank.id, 0),
    ];
    insert(
      &old,
      vec![
        none, queued, completed, unknown, no_end, blank, running, done,
      ],
    )
    .await;
    old
      .write(move |conn| {
        for (id, state) in raw {
          conn.execute(
            "UPDATE Transfers SET State = ?2 WHERE Id = ?1",
            rusqlite::params![id.to_string(), state],
          )?;
        }
        conn.execute(
          "UPDATE Transfers SET RequestedAt = '' WHERE Id = ?1",
          [blank_id.to_string()],
        )?;
        // the queued row was last heard of when the peer queued it
        store::entered(conn, lost[1].0, TransferStates::QueuedRemotely, at(7))
      })
      .await
      .unwrap();
    drop(old);
    let before = chrono::Utc::now();
    let migrated = open(&location).await.unwrap();
    let errored = Completed(Errored);
    let read = |id: uuid::Uuid| {
      migrated.read(move |conn| Ok((store::get(conn, id)?.unwrap(), store::history(conn, id)?)))
    };
    for (id, ended) in lost {
      let (record, history) = read(id).await.unwrap();
      assert_eq!(
        (
          record.state.kind(),
          record.ended_at,
          record.exception.as_deref()
        ),
        (errored, Some(ended), Some("State lost")),
        "{}",
        record.username
      );
      let last = history.last().unwrap();
      assert_eq!(
        (last.state, last.at),
        (errored, ended),
        "{}",
        record.username
      );
    }
    for (id, state) in kept {
      let (record, history) = read(id).await.unwrap();
      assert_eq!(
        (record.state.kind(), record.exception),
        (state, None),
        "{}",
        record.username
      );
      assert!(history.is_empty(), "{}", record.username);
    }
    let (state, ended): (u32, String) = migrated
      .read(move |conn| {
        conn.query_row(
          "SELECT State, EndedAt FROM Transfers WHERE Id = ?1",
          [blank_id.to_string()],
          |row| Ok((row.get(0)?, row.get(1)?)),
        )
      })
      .await
      .unwrap();
    assert_eq!(state::from_column(state), Some(errored));
    let ended = db::read_timestamp(&ended).unwrap();
    assert!(
      ended >= before - chrono::Duration::seconds(1),
      "a row that holds no time ends when it is migrated"
    );
  }
}
