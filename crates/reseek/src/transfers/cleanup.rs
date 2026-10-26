//! Ending the transfers a daemon that stopped without finishing them left behind.

use api_types::transfers::{TransferDirection, TransferStates};
use chrono::Utc;
use rusqlite::params;
use tracing::{debug, info};
use uuid::Uuid;

use super::direction_at;
use crate::db::{Database, DbError, timestamp};

/// Why a transfer left unfinished at the last stop ended.
pub const SHUT_DOWN: &str = "Application shut down";

/// Which transfers are unfinished: not ended, or in a state that is not complete.
const UNFINISHED: &str = "EndedAt IS NULL OR (State & 16) = 0";

/// Ends every transfer left unfinished, removed ones included, as `Completed, Errored` with
/// [`SHUT_DOWN`] at the time of this call. Returns the ids of the downloads among them, which
/// were active when the daemon last stopped.
pub async fn end_unfinished(db: &Database) -> Result<Vec<Uuid>, DbError> {
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
  let mut downloads = Vec::new();
  for (id, direction) in &ended {
    debug!(
      id,
      ?direction,
      "transfer left unfinished at the last stop ended"
    );
    if *direction == TransferDirection::Download {
      // ids are written by the transfer engines as UUIDs; another one is not resumed
      if let Ok(id) = id.parse() {
        downloads.push(id);
      }
    }
  }
  if !ended.is_empty() {
    info!(
      transfers = ended.len(),
      downloads = downloads.len(),
      "transfers left unfinished at the last stop ended"
    );
  }
  Ok(downloads)
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
    let mut downloads = end_unfinished(&db).await.unwrap();
    downloads.sort();
    let mut expected = vec![queued.id, removed.id, unended.id];
    expected.sort();
    assert_eq!(downloads, expected);
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
    assert!(
      end_unfinished(&db).await.unwrap().is_empty(),
      "nothing is left"
    );
  }
}
