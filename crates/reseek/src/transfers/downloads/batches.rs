//! Batches: downloads from one user enqueued together, with an optional destination of their
//! own and a tag. A batch is written once, with the files it was enqueued with, and never
//! changed; what it has done so far is counted from its downloads when it is read.

use api_types::transfers::{
  Batch, BatchOptions, BatchState, EnqueueDownloadBatchItem, Transfer, TransferDirection,
  TransferStates,
};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, ErrorCode, OptionalExtension, params};
use uuid::Uuid;

use crate::db::{Database, DbError, read_timestamp, timestamp};
use crate::transfers::{COLUMNS, direction_text, transfer};

/// A batch as its row holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchRow {
  /// the batch's id
  pub id: Uuid,
  /// the search its files were found by
  pub search_id: Option<Uuid>,
  /// the user its files come from
  pub username: String,
  /// when it was made
  pub created_at: DateTime<Utc>,
  /// its destination and tag
  pub options: BatchOptions,
}

/// Why a batch was not written.
#[derive(Debug, thiserror::Error)]
pub enum InsertError {
  /// a batch with its id exists
  #[error("A batch with ID {0} already exists")]
  Exists(Uuid),
  /// the database failed
  #[error(transparent)]
  Db(#[from] rusqlite::Error),
}

/// Writes `batch` with the files it was enqueued with.
pub fn insert(
  conn: &Connection,
  batch: &BatchRow,
  files: &[EnqueueDownloadBatchItem],
) -> Result<(), InsertError> {
  let options = serde_json::to_string(&batch.options).expect("batch options are plain JSON");
  let files = serde_json::to_string(files).expect("batch files are plain JSON");
  let written = conn.execute(
    "INSERT INTO Batches (Id, SearchId, Username, Direction, CreatedAt, Options, files_json) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    params![
      batch.id.to_string(),
      batch.search_id.map(|id| id.to_string()),
      batch.username,
      direction_text(TransferDirection::Download),
      timestamp(batch.created_at),
      options,
      files,
    ],
  );
  match written {
    Ok(_) => Ok(()),
    Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == ErrorCode::ConstraintViolation => {
      Err(InsertError::Exists(batch.id))
    }
    Err(e) => Err(e.into()),
  }
}

/// The batch `id`.
pub fn get(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<BatchRow>> {
  conn
    .query_row(
      "SELECT Id, SearchId, Username, CreatedAt, Options FROM Batches WHERE Id = ?1",
      [id.to_string()],
      |row| {
        let uuid = |i: usize, text: String| {
          text.parse::<Uuid>().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, e.into())
          })
        };
        let options = row
          .get::<_, Option<String>>(4)?
          .map(|text| {
            serde_json::from_str(&text).map_err(|e| {
              rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, e.into())
            })
          })
          .transpose()?
          .unwrap_or_default();
        Ok(BatchRow {
          id: uuid(0, row.get(0)?)?,
          search_id: row
            .get::<_, Option<String>>(1)?
            .map(|text| uuid(1, text))
            .transpose()?,
          username: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
          created_at: read_timestamp(&row.get::<_, String>(3)?)?,
          options,
        })
      },
    )
    .optional()
}

/// The batch `id` with its downloads in the order they were asked for, removed ones only with
/// `include_removed`, and what they have done counted.
pub async fn read(
  db: &Database,
  id: Uuid,
  include_removed: bool,
) -> Result<Option<Batch>, DbError> {
  db.read(move |conn| {
    let Some(row) = get(conn, id)? else {
      return Ok(None);
    };
    let mut statement = conn.prepare_cached(&format!(
      "SELECT {COLUMNS} FROM Transfers WHERE BatchId = ?1 AND (?2 OR Removed = 0) \
       ORDER BY RequestedAt, Id"
    ))?;
    let transfers = statement
      .query_map(params![id.to_string(), include_removed], transfer)?
      .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(batch(row, transfers)))
  })
  .await
}

/// The batch of `row` holding `transfers`, with their counts and where they are as a whole.
fn batch(row: BatchRow, transfers: Vec<Transfer>) -> Batch {
  let completed = |t: &&Transfer| t.state.contains(TransferStates::Completed);
  let succeeded = TransferStates::Completed | TransferStates::Succeeded;
  let completed_count = transfers.iter().filter(completed).count();
  let failed_count = transfers
    .iter()
    .filter(completed)
    .filter(|t| t.state != succeeded)
    .count();
  let started = transfers.iter().any(|t| {
    t.state.intersects(
      TransferStates::Initializing | TransferStates::InProgress | TransferStates::Completed,
    )
  });
  let state = if transfers.is_empty() || !started {
    BatchState::Pending
  } else if completed_count < transfers.len() {
    BatchState::InProgress
  } else if failed_count > 0 {
    BatchState::Failed
  } else {
    BatchState::Completed
  };
  let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
  Batch {
    id: row.id,
    search_id: row.search_id,
    username: row.username,
    direction: TransferDirection::Download,
    created_at: row.created_at,
    file_count: count(transfers.len()),
    completed_count: count(completed_count),
    failed_count: count(failed_count),
    bytes_total: transfers.iter().map(|t| t.size).sum(),
    bytes_transferred: transfers.iter().map(|t| t.bytes_transferred).sum(),
    state,
    transfers,
    options: row.options,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::Record;
  use crate::transfers::fixtures::transfers;
  use crate::transfers::store;

  fn row() -> BatchRow {
    BatchRow {
      id: Uuid::new_v4(),
      search_id: Some(Uuid::new_v4()),
      username: "alice".to_string(),
      created_at: "2026-05-06T10:00:00Z".parse().unwrap(),
      options: BatchOptions {
        destination: Some("Artist/Album".to_string()),
        external_id: Some("abc".to_string()),
      },
    }
  }

  #[tokio::test]
  async fn a_batch_reads_back_with_its_options_and_a_known_id_is_refused() {
    let db = transfers().await;
    let batch_row = row();
    let written = batch_row.clone();
    db.write(move |conn| Ok(insert(conn, &written, &[])))
      .await
      .unwrap()
      .unwrap();
    let again = batch_row.clone();
    let refused = db
      .write(move |conn| Ok(insert(conn, &again, &[])))
      .await
      .unwrap()
      .unwrap_err();
    assert_eq!(
      refused.to_string(),
      format!("A batch with ID {} already exists", batch_row.id)
    );
    let id = batch_row.id;
    let read = db.read(move |conn| get(conn, id)).await.unwrap().unwrap();
    assert_eq!(read, batch_row);
  }

  #[tokio::test]
  async fn a_batch_counts_its_downloads_and_leaves_out_removed_ones() {
    let db = transfers().await;
    let batch_row = row();
    let id = batch_row.id;
    let record = |state: TransferStates, removed: bool, size: u64| {
      let mut record = Record::download("alice", &Uuid::new_v4().to_string(), size, Some(id));
      record.state = state;
      record.removed = removed;
      record.bytes_transferred = size;
      record
    };
    let done = TransferStates::Completed | TransferStates::Succeeded;
    let records = vec![
      record(done, false, 10),
      record(
        TransferStates::Completed | TransferStates::Rejected,
        false,
        5,
      ),
      record(done, true, 100),
    ];
    db.write(move |conn| {
      insert(conn, &batch_row, &[]).map_err(|e| match e {
        InsertError::Db(e) => e,
        InsertError::Exists(_) => unreachable!("a fresh database"),
      })?;
      for record in &records {
        store::insert(conn, record)?;
      }
      Ok(())
    })
    .await
    .unwrap();
    let batch = read(&db, id, false).await.unwrap().unwrap();
    assert_eq!(
      (batch.file_count, batch.completed_count, batch.failed_count),
      (2, 2, 1)
    );
    assert_eq!((batch.bytes_total, batch.bytes_transferred), (15, 15));
    assert_eq!(batch.state, BatchState::Failed);
    let all = read(&db, id, true).await.unwrap().unwrap();
    assert_eq!(all.file_count, 3);
    assert!(read(&db, Uuid::new_v4(), false).await.unwrap().is_none());
  }

  #[test]
  fn a_batch_is_pending_until_a_download_starts() {
    let queued = |state| {
      let mut record = Record::download("alice", "a", 1, None);
      record.state = state;
      record.transfer()
    };
    let state =
      |states: &[TransferStates]| batch(row(), states.iter().map(|s| queued(*s)).collect()).state;
    let locally = TransferStates::Queued | TransferStates::Locally;
    assert_eq!(state(&[]), BatchState::Pending);
    assert_eq!(
      state(&[locally, TransferStates::Requested]),
      BatchState::Pending
    );
    assert_eq!(
      state(&[locally, TransferStates::InProgress]),
      BatchState::InProgress
    );
    let done = TransferStates::Completed | TransferStates::Succeeded;
    assert_eq!(state(&[done, locally]), BatchState::InProgress);
    assert_eq!(state(&[done, done]), BatchState::Completed);
  }
}
