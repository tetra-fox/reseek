//! Ending the transfers left unfinished when the daemon stops, or when it stopped without
//! ending them, and taking the downloads among them to resume. A download ended here is marked
//! resumable; the first login after a start takes the marked downloads not removed since and
//! clears every mark, so each is resumed once at most.

use std::collections::BTreeMap;

use api_types::transfers::{TransferDirection, TransferStates};
use chrono::Utc;
use rusqlite::params;
use tracing::{debug, info};
use uuid::Uuid;

use super::categories::UNFINISHED;
use super::downloads::{FileRequest, Group};
use super::{ASKED_ORDER, COLUMNS, Record, direction_at, direction_text};
use crate::db::{Database, DbError, timestamp};

/// Why a transfer left unfinished at the last stop ended.
pub const SHUT_DOWN: &str = "Application shut down";

/// The downloads to resume by job, in groups of one user, destination and search each, in the
/// order they were asked for; downloads from before jobs were kept are under none.
pub type Resumable = BTreeMap<Option<Uuid>, Vec<Group>>;

/// Ends every transfer left unfinished but the paused downloads, removed ones included, as
/// `Completed, Errored` with [`SHUT_DOWN`] at the time of this call, adding the state to each
/// one's history and marking the downloads among them resumable. Returns how many it ended.
pub async fn end_unfinished(db: &Database) -> Result<usize, DbError> {
  let errored = TransferStates::Completed | TransferStates::Errored;
  let now = timestamp(Utc::now());
  let ended = db
    .write(move |conn| {
      let tx = conn.transaction()?;
      let unfinished = {
        let mut select = tx.prepare(&format!(
          "SELECT Id, Direction FROM Transfers WHERE {UNFINISHED} AND Paused = 0"
        ))?;
        select
          .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, direction_at(row, 1)?))
          })?
          .collect::<rusqlite::Result<Vec<_>>>()?
      };
      tx.execute(
        &format!(
          "INSERT INTO transfers_state_history (transfer_id, state, at) SELECT Id, ?1, ?2 \
           FROM Transfers WHERE {UNFINISHED} AND Paused = 0"
        ),
        params![errored.bits(), now],
      )?;
      tx.execute(
        &format!(
          "UPDATE Transfers SET State = ?1, StateDescription = ?2, EndedAt = ?3, \
           Exception = ?4, Resumable = (Direction = ?5 OR Resumable), NextAttemptAt = NULL, \
           PlaceInQueue = NULL WHERE {UNFINISHED} AND Paused = 0"
        ),
        params![
          errored.bits(),
          errored.to_string(),
          now,
          SHUT_DOWN,
          direction_text(TransferDirection::Download),
        ],
      )?;
      tx.commit()?;
      Ok(unfinished)
    })
    .await?;
  for (id, direction) in &ended {
    debug!(id, ?direction, "unfinished transfer ended");
  }
  if !ended.is_empty() {
    info!(transfers = ended.len(), "unfinished transfers ended");
  }
  Ok(ended.len())
}

/// Takes the downloads marked resumable that were not removed since, as files to enqueue
/// grouped by job, then by user, destination and search, each keeping its place in the local
/// queue, and clears the mark of every marked download, removed or not.
pub async fn take_resumable(db: &Database) -> Result<Resumable, DbError> {
  db.write(move |conn| {
    let tx = conn.transaction()?;
    let mut jobs = Resumable::new();
    {
      let mut select = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM Transfers WHERE Resumable = 1 AND Direction = ?1 AND Removed = 0 \
         ORDER BY {ASKED_ORDER}"
      ))?;
      let records = select.query_map(
        params![direction_text(TransferDirection::Download)],
        Record::read,
      )?;
      for record in records {
        let record = record?;
        let file = FileRequest {
          position: record.queue_position,
          ..FileRequest::of(&record)
        };
        let groups = jobs.entry(record.job_id).or_default();
        match groups.iter_mut().find(|group| group.holds(&record)) {
          Some(group) => group.files.push(file),
          None => groups.push(Group {
            files: vec![file],
            ..Group::of(&record)
          }),
        }
      }
    }
    tx.execute("UPDATE Transfers SET Resumable = 0 WHERE Resumable = 1", [])?;
    tx.commit()?;
    Ok(jobs)
  })
  .await
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::fixtures::{Row, insert, transfers};
  use crate::transfers::{Record, store};

  #[tokio::test]
  async fn unfinished_transfers_end_errored_and_finished_ones_stay() {
    let db = transfers().await;
    let queued = Row::new("alice", TransferDirection::Download, 2 | 4096);
    let removed = Row {
      removed: true,
      ..Row::new("alice", TransferDirection::Download, 8)
    };
    let uploading = Row::new("bob", TransferDirection::Upload, 8);
    let done = Row {
      ended_at: Some(Utc::now()),
      ..Row::new("carol", TransferDirection::Download, 48)
    };
    let unended = Row::new("dave", TransferDirection::Download, 48);
    insert(
      &db,
      vec![
        queued.clone(),
        removed.clone(),
        uploading.clone(),
        done.clone(),
        unended.clone(),
      ],
    )
    .await;
    assert_eq!(end_unfinished(&db).await.unwrap(), 4);
    let rows: Vec<(String, u32, Option<String>, bool)> = db
      .read(|conn| {
        conn
          .prepare("SELECT Id, State, Exception, EndedAt IS NOT NULL FROM Transfers")?
          .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
          .collect()
      })
      .await
      .unwrap();
    for (id, state, exception, ended) in rows {
      assert!(ended, "{id}");
      if id == done.id.to_string() {
        assert_eq!((state, exception), (48, None));
      } else {
        assert_eq!(
          (state, exception.as_deref()),
          (272, Some(SHUT_DOWN)),
          "{id}"
        );
      }
    }
    for row in [&queued, &removed, &uploading, &unended] {
      let id = row.id;
      let history = db.read(move |conn| store::history(conn, id)).await.unwrap();
      let states: Vec<u32> = history.iter().map(|change| change.state.bits()).collect();
      assert_eq!(states, [272], "{id}");
    }
    let id = done.id;
    let history = db.read(move |conn| store::history(conn, id)).await.unwrap();
    assert!(history.is_empty(), "a finished transfer gets no entry");
    assert_eq!(end_unfinished(&db).await.unwrap(), 0, "nothing is left");
    let marked: Vec<String> = db
      .read(|conn| {
        conn
          .prepare("SELECT Id FROM Transfers WHERE Resumable = 1 ORDER BY Id")?
          .query_map([], |row| row.get(0))?
          .collect()
      })
      .await
      .unwrap();
    let mut expected: Vec<String> = [queued.id, removed.id, unended.id]
      .iter()
      .map(Uuid::to_string)
      .collect();
    expected.sort();
    assert_eq!(marked, expected, "the downloads ended, removed or not");
  }

  #[tokio::test]
  async fn the_marked_downloads_are_taken_once_and_every_mark_is_cleared() {
    let db = transfers().await;
    let mut kept = Row::new("alice", TransferDirection::Download, 2 | 2048);
    kept.filename = "a\\one.flac".to_string();
    let removed = Row {
      removed: true,
      ..Row::new("alice", TransferDirection::Download, 8)
    };
    insert(&db, vec![kept, removed]).await;
    end_unfinished(&db).await.unwrap();
    let taken = take_resumable(&db).await.unwrap();
    let files: Vec<(&str, &str)> = taken
      .values()
      .flatten()
      .flat_map(|group| {
        group
          .files
          .iter()
          .map(|f| (group.username.as_str(), f.filename.as_str()))
      })
      .collect();
    assert_eq!(
      files,
      [("alice", "a\\one.flac")],
      "only what was not removed"
    );
    assert!(
      take_resumable(&db).await.unwrap().is_empty(),
      "resumed once"
    );
    let marks: i64 = db
      .read(|conn| conn.query_row("SELECT SUM(Resumable) FROM Transfers", [], |r| r.get(0)))
      .await
      .unwrap();
    assert_eq!(marks, 0);
  }

  #[tokio::test]
  async fn interrupted_downloads_come_back_in_their_job_with_their_destination_search_and_place() {
    let db = transfers().await;
    let job = Uuid::new_v4();
    let search = Uuid::new_v4();
    let in_job = |filename: &str, destination: &str| Record {
      job_id: Some(job),
      destination: Some(destination.to_string()),
      search_id: Some(search),
      ..Record::download("alice", filename, 1)
    };
    let mut records = [
      in_job("a\\Disc 1\\1.flac", "Album/Disc 1"),
      in_job("a\\Disc 2\\1.flac", "Album/Disc 2"),
      in_job("a\\Disc 1\\2.flac", "Album/Disc 1"),
      Record::download("alice", "b\\old.flac", 1),
    ];
    // asked for a second apart, so the order they are taken in is the order above, and placed
    // in the local queue the other way round
    let asked: chrono::DateTime<Utc> = "2026-05-06T10:00:00Z".parse().unwrap();
    for (record, seconds) in records.iter_mut().zip(0..) {
      record.requested_at = asked + chrono::Duration::seconds(seconds);
      record.queue_position = Some(4.0 - seconds as f64);
    }
    db.write(move |conn| {
      for record in &records {
        store::insert(conn, record)?;
      }
      Ok(())
    })
    .await
    .unwrap();
    end_unfinished(&db).await.unwrap();
    let resumable = take_resumable(&db).await.unwrap();
    let group = |destination: Option<&str>, search_id, files: &[(&str, f64)]| Group {
      username: "alice".to_string(),
      files: files
        .iter()
        .map(|(filename, position)| FileRequest {
          filename: (*filename).to_string(),
          size: 1,
          position: Some(*position),
        })
        .collect(),
      destination: destination.map(str::to_string),
      search_id,
    };
    let expected = Resumable::from([
      (
        Some(job),
        vec![
          group(
            Some("Album/Disc 1"),
            Some(search),
            &[("a\\Disc 1\\1.flac", 4.0), ("a\\Disc 1\\2.flac", 2.0)],
          ),
          group(
            Some("Album/Disc 2"),
            Some(search),
            &[("a\\Disc 2\\1.flac", 3.0)],
          ),
        ],
      ),
      (None, vec![group(None, None, &[("b\\old.flac", 1.0)])]),
    ]);
    assert_eq!(
      resumable, expected,
      "each destination keeps its files and each file its place, and a download from before \
       jobs comes back under none"
    );
  }
}
