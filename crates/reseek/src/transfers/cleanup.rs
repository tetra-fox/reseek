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
use super::downloads::FileRequest;
use super::{direction_at, direction_text, text_at};
use crate::db::{Database, DbError, timestamp};

/// Why a transfer left unfinished at the last stop ended.
pub const SHUT_DOWN: &str = "Application shut down";

/// The downloads to resume: the files of each user, batch and job.
pub type Resumable = BTreeMap<(String, Option<Uuid>, Option<Uuid>), Vec<FileRequest>>;

/// Ends every transfer left unfinished, removed ones included, as `Completed, Errored` with
/// [`SHUT_DOWN`] at the time of this call, marking the downloads among them resumable. Returns
/// how many it ended.
pub async fn end_unfinished(db: &Database) -> Result<usize, DbError> {
  let errored = TransferStates::Completed | TransferStates::Errored;
  let ended = db
    .write(move |conn| {
      let tx = conn.transaction()?;
      let unfinished = {
        let mut select = tx.prepare(&format!(
          "SELECT Id, Direction FROM Transfers WHERE {UNFINISHED}"
        ))?;
        select
          .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, direction_at(row, 1)?))
          })?
          .collect::<rusqlite::Result<Vec<_>>>()?
      };
      tx.execute(
        &format!(
          "UPDATE Transfers SET State = ?1, StateDescription = ?2, EndedAt = ?3, \
           Exception = ?4, Resumable = (Direction = ?5 OR Resumable), NextAttemptAt = NULL, \
           PlaceInQueue = NULL WHERE {UNFINISHED}"
        ),
        params![
          errored.bits(),
          errored.to_string(),
          timestamp(Utc::now()),
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
/// grouped by user, batch and job, and clears the mark of every marked download, removed or not.
pub async fn take_resumable(db: &Database) -> Result<Resumable, DbError> {
  db.write(move |conn| {
    let tx = conn.transaction()?;
    let mut groups = Resumable::new();
    {
      let mut select = tx.prepare(
        "SELECT Username, BatchId, Filename, Size, JobId FROM Transfers WHERE Resumable = 1 \
         AND Direction = ?1 AND Removed = 0 ORDER BY RequestedAt, Id",
      )?;
      let rows = select.query_map(
        params![direction_text(TransferDirection::Download)],
        |row| {
          Ok((
            text_at(row, 0)?,
            row.get::<_, Option<String>>(1)?,
            text_at(row, 2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<String>>(4)?,
          ))
        },
      )?;
      for row in rows {
        let (username, batch, filename, size, job) = row?;
        // batch and job ids are written by the download engine as UUIDs; another one resumes
        // alone
        let batch = batch.and_then(|id| id.parse().ok());
        let job = job.and_then(|id| id.parse().ok());
        groups
          .entry((username, batch, job))
          .or_default()
          .push(FileRequest {
            filename,
            size: size.max(0).unsigned_abs(),
          });
      }
    }
    tx.execute("UPDATE Transfers SET Resumable = 0 WHERE Resumable = 1", [])?;
    tx.commit()?;
    Ok(groups)
  })
  .await
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::fixtures::{Row, insert, transfers};

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
      .iter()
      .flat_map(|((user, _, _), files)| {
        files
          .iter()
          .map(move |f| (user.as_str(), f.filename.as_str()))
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
}
