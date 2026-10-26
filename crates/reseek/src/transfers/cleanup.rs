//! Ending the transfers left unfinished when the daemon stops, or when it stopped without
//! ending them, and finding the downloads among them to resume.

use std::collections::BTreeMap;

use api_types::transfers::{TransferDirection, TransferStates};
use chrono::Utc;
use rusqlite::params;
use tracing::{debug, info};
use uuid::Uuid;

use super::categories::UNFINISHED;
use super::downloads::FileRequest;
use super::{direction_at, direction_text};
use crate::db::{Database, DbError, timestamp};

/// Why a transfer left unfinished at the last stop ended.
pub const SHUT_DOWN: &str = "Application shut down";

/// Ends every transfer left unfinished, removed ones included, as `Completed, Errored` with
/// [`SHUT_DOWN`] at the time of this call. Returns how many it ended.
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
           Exception = ?4 WHERE {UNFINISHED}"
        ),
        params![
          errored.bits(),
          errored.to_string(),
          timestamp(Utc::now()),
          SHUT_DOWN,
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

/// The downloads a stop ended, as [`end_unfinished`] ends them, that were not removed since:
/// the ones to resume.
pub async fn resumable(db: &Database) -> Result<Vec<Uuid>, DbError> {
  let errored = TransferStates::Completed | TransferStates::Errored;
  let rows = db
    .read(move |conn| {
      let mut statement = conn.prepare(
        "SELECT Id FROM Transfers WHERE Direction = ?1 AND State = ?2 AND Exception = ?3 \
         AND Removed = 0 ORDER BY RequestedAt, Id",
      )?;
      let rows = statement.query_map(
        params![
          direction_text(TransferDirection::Download),
          errored.bits(),
          SHUT_DOWN
        ],
        |row| row.get::<_, String>(0),
      )?;
      rows.collect::<rusqlite::Result<Vec<String>>>()
    })
    .await?;
  // ids are written by the transfer engines as UUIDs; another one is not resumed
  Ok(rows.iter().filter_map(|id| id.parse().ok()).collect())
}

/// The downloads of `ids` still not removed, as files to enqueue, grouped by user and batch.
pub async fn resume_groups(
  db: &Database,
  ids: Vec<Uuid>,
) -> Result<BTreeMap<(String, Option<Uuid>), Vec<FileRequest>>, DbError> {
  db.read(move |conn| {
    let mut groups: BTreeMap<(String, Option<Uuid>), Vec<FileRequest>> = BTreeMap::new();
    for id in ids {
      let Some(record) = super::store::get(conn, id)? else {
        continue;
      };
      if record.removed {
        continue;
      }
      groups
        .entry((record.username, record.batch_id))
        .or_default()
        .push(FileRequest {
          filename: record.filename,
          size: record.size,
        });
    }
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
    let mut resumed = resumable(&db).await.unwrap();
    resumed.sort();
    let mut expected = vec![queued.id, unended.id];
    expected.sort();
    assert_eq!(resumed, expected, "downloads not removed");
  }
}
