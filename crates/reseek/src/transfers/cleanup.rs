//! Ending the transfers left unfinished when the daemon stops, or when it stopped without
//! ending them, and taking the downloads among them to resume. A download ended here is marked
//! resumable. The mark is what says a download waits for a login to be enqueued again: a login
//! takes the marked downloads not removed since and clears each one's mark once it was tried,
//! so the files refused for want of the server stay marked for the next login, across a stop
//! too, and a cancel of their job clears them.

use std::collections::BTreeMap;

use api_types::transfers::{TransferDirection, TransferOutcome, TransferStates};
use chrono::Utc;
use rusqlite::params;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::categories::UNFINISHED;
use super::downloads::{FileRequest, Group};
use super::state::column;
use super::{ASKED_ORDER, COLUMNS, Record, direction_at, direction_text};
use crate::db::{Database, DbError, timestamp};

/// Why a transfer left unfinished at the last stop ended.
pub const SHUT_DOWN: &str = "Application shut down";

/// The downloads to resume by job, in groups of one user, destination and search each, in the
/// order they were asked for.
pub type Resumable = BTreeMap<Uuid, Vec<Group>>;

/// Ends every transfer left unfinished but the paused downloads, removed ones included, as
/// `Completed, Errored` with [`SHUT_DOWN`] at the time of this call, adding the state to each
/// one's history and marking the downloads among them resumable. Returns how many it ended.
pub async fn end_unfinished(db: &Database) -> Result<usize, DbError> {
  let errored = TransferStates::Completed(TransferOutcome::Errored);
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
        params![column(errored), now],
      )?;
      tx.execute(
        &format!(
          "UPDATE Transfers SET State = ?1, StateDescription = ?2, EndedAt = ?3, \
           Exception = ?4, Resumable = (Direction = ?5 OR Resumable), NextAttemptAt = NULL, \
           PlaceInQueue = NULL WHERE {UNFINISHED} AND Paused = 0"
        ),
        params![
          column(errored),
          errored.as_str(),
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

/// The downloads marked resumable that were not removed since, as files to enqueue grouped by
/// job, then by user, destination and search, each keeping its place in the local queue. Their
/// marks stay until each file was tried, [`clear_resumable`]; the marks of downloads that can no
/// longer be resumed, removed ones and ones without a job, which are logged, are cleared here.
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
        let Some(job) = record.job_id else {
          warn!(id = %record.id, username = record.username, "interrupted download without a job not resumed");
          continue;
        };
        let file = FileRequest {
          position: record.queue_position,
          ..FileRequest::of(&record)
        };
        let groups = jobs.entry(job).or_default();
        match groups.iter_mut().find(|group| group.holds(&record)) {
          Some(group) => group.files.push(file),
          None => groups.push(Group {
            files: vec![file],
            ..Group::of(&record)
          }),
        }
      }
    }
    tx.execute(
      "UPDATE Transfers SET Resumable = 0 WHERE Resumable = 1 AND (Removed = 1 OR JobId IS NULL)",
      [],
    )?;
    tx.commit()?;
    Ok(jobs)
  })
  .await
}

/// Clears the marks of the files `filenames` of `username` in the job `job`, which were tried,
/// whether or not they could be enqueued again.
pub async fn clear_resumable(
  db: &Database,
  job: Uuid,
  username: &str,
  filenames: Vec<String>,
) -> Result<(), DbError> {
  let username = username.to_string();
  db.write(move |conn| {
    let tx = conn.transaction()?;
    {
      let mut clear = tx.prepare(
        "UPDATE Transfers SET Resumable = 0 WHERE JobId = ?1 AND Username = ?2 \
         AND Direction = ?3 AND Filename = ?4 AND Resumable = 1",
      )?;
      for filename in &filenames {
        clear.execute(params![
          job.to_string(),
          username,
          direction_text(TransferDirection::Download),
          filename
        ])?;
      }
    }
    tx.commit()
  })
  .await
}

/// Clears the marks of the downloads of the job `job`, whose files are given up; returns
/// whether any that was still listed waited to be resumed.
pub async fn forget_resumable(db: &Database, job: Uuid) -> Result<bool, DbError> {
  db.write(move |conn| {
    let tx = conn.transaction()?;
    let waited = tx.execute(
      "UPDATE Transfers SET Resumable = 0 WHERE JobId = ?1 AND Resumable = 1 AND Removed = 0",
      [job.to_string()],
    )?;
    tx.execute(
      "UPDATE Transfers SET Resumable = 0 WHERE JobId = ?1 AND Resumable = 1",
      [job.to_string()],
    )?;
    tx.commit()?;
    Ok(waited > 0)
  })
  .await
}

/// Whether any download still listed waits to be resumed.
pub async fn any_resumable(db: &Database) -> Result<bool, DbError> {
  db.read(|conn| {
    conn.query_row(
      "SELECT EXISTS (SELECT 1 FROM Transfers WHERE Resumable = 1 AND Removed = 0 \
       AND Direction = 'Download' AND JobId IS NOT NULL)",
      [],
      |row| row.get(0),
    )
  })
  .await
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::fixtures::{Row, insert, transfers};
  use crate::transfers::state::state_at;
  use crate::transfers::{Record, store};

  /// the state a transfer left unfinished ends in
  const STOPPED: TransferStates = TransferStates::Completed(TransferOutcome::Errored);

  #[tokio::test]
  async fn unfinished_transfers_end_errored_and_finished_ones_stay() {
    let db = transfers().await;
    let queued = Row::new(
      "alice",
      TransferDirection::Download,
      TransferStates::QueuedRemotely,
    );
    let removed = Row {
      removed: true,
      ..Row::new(
        "alice",
        TransferDirection::Download,
        TransferStates::InProgress,
      )
    };
    let uploading = Row::new("bob", TransferDirection::Upload, TransferStates::InProgress);
    let done = Row {
      ended_at: Some(Utc::now()),
      ..Row::new(
        "carol",
        TransferDirection::Download,
        TransferStates::Completed(TransferOutcome::Succeeded),
      )
    };
    insert(
      &db,
      vec![
        queued.clone(),
        removed.clone(),
        uploading.clone(),
        done.clone(),
      ],
    )
    .await;
    assert_eq!(end_unfinished(&db).await.unwrap(), 3);
    let rows: Vec<(String, TransferStates, Option<String>, bool)> = db
      .read(|conn| {
        conn
          .prepare("SELECT Id, State, Exception, EndedAt IS NOT NULL FROM Transfers")?
          .query_map([], |r| {
            Ok((r.get(0)?, state_at(r, 1)?, r.get(2)?, r.get(3)?))
          })?
          .collect()
      })
      .await
      .unwrap();
    for (id, state, exception, ended) in rows {
      assert!(ended, "{id}");
      if id == done.id.to_string() {
        assert_eq!((state, exception), (done.state, None));
      } else {
        assert_eq!(
          (state, exception.as_deref()),
          (STOPPED, Some(SHUT_DOWN)),
          "{id}"
        );
      }
    }
    for row in [&queued, &removed, &uploading] {
      let id = row.id;
      let history = db.read(move |conn| store::history(conn, id)).await.unwrap();
      let states: Vec<TransferStates> = history.iter().map(|change| change.state).collect();
      assert_eq!(states, [STOPPED], "{id}");
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
    let mut expected: Vec<String> = [queued.id, removed.id]
      .iter()
      .map(Uuid::to_string)
      .collect();
    expected.sort();
    assert_eq!(marked, expected, "the downloads ended, removed or not");
  }

  #[tokio::test]
  async fn a_marked_download_is_taken_until_it_was_tried_and_a_removed_ones_mark_is_cleared() {
    let db = transfers().await;
    let mut kept = Row::new(
      "alice",
      TransferDirection::Download,
      TransferStates::QueuedLocally,
    );
    kept.filename = "a\\one.flac".to_string();
    let removed = Row {
      removed: true,
      ..Row::new(
        "alice",
        TransferDirection::Download,
        TransferStates::InProgress,
      )
    };
    insert(&db, vec![kept, removed]).await;
    let job = Uuid::new_v4();
    let text = job.to_string();
    db.write(move |conn| conn.execute("UPDATE Transfers SET JobId = ?1", [text]))
      .await
      .unwrap();
    end_unfinished(&db).await.unwrap();
    let files = |taken: Resumable| -> Vec<(String, String)> {
      taken
        .into_values()
        .flatten()
        .flat_map(|group| {
          let username = group.username;
          group
            .files
            .into_iter()
            .map(move |f| (username.clone(), f.filename))
        })
        .collect()
    };
    let only = [("alice".to_string(), "a\\one.flac".to_string())];
    assert_eq!(
      files(take_resumable(&db).await.unwrap()),
      only,
      "only what was not removed"
    );
    assert_eq!(
      files(take_resumable(&db).await.unwrap()),
      only,
      "kept until tried"
    );
    assert!(any_resumable(&db).await.unwrap());
    clear_resumable(&db, job, "alice", vec!["a\\one.flac".to_string()])
      .await
      .unwrap();
    assert!(take_resumable(&db).await.unwrap().is_empty());
    assert!(!any_resumable(&db).await.unwrap());
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
      Record::download("alice", "b\\jobless.flac", 1),
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
    let expected = Resumable::from([(
      job,
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
    )]);
    assert_eq!(
      resumable, expected,
      "each destination keeps its files and each file its place, and a download without a job \
       is left"
    );
  }
}
