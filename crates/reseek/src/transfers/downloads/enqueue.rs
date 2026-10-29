//! Enqueueing downloads: the request and the peer are checked, then, in the user's turn, a
//! record is written for every file not downloading already, in one transaction with the
//! batch the files belong to, and a task is started for each. A record for a file that
//! finished earlier is superseded by the new one.

use std::collections::HashSet;
use std::sync::Arc;

use api_types::live::LiveFrame;
use api_types::transfers::{EnqueueDownloadBatchItem, EnqueueFailure, Transfer, TransferDirection};
use api_types::users::blocking::BlockScope;
use rusqlite::params;
use soulseek_client::ClientError;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::batches::{self, BatchRow, InsertError};
use super::{Downloads, InFlight};
use crate::api::error::ApiError;
use crate::safety::traverses;
use crate::sync::lock;
use crate::transfers::categories::UNFINISHED;
use crate::transfers::removal::{self, Removed};
use crate::transfers::{Record, direction_text, store};
use crate::users::Touch;

/// Up to this many files, every one of a request asks its peer at once; more than this, five
/// at a time, since some peers drop connections asked for dozens of files at once.
const ALL_AT_ONCE: usize = 30;
/// How many files of a larger request ask their peer at once.
const AT_ONCE: usize = 5;

/// What scripts read as a file already downloading.
const ALREADY_RUNNING: &str = "Skipped: Already in progress";

/// One file to download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRequest {
  /// the remote filename exactly as the peer shares it
  pub filename: String,
  /// its size in bytes, 0 when not known
  pub size: u64,
}

/// A batch to make with the files, and the files as the request gave them.
pub struct NewBatch {
  /// the batch
  pub row: BatchRow,
  /// the files with their attributes, kept with the batch
  pub files: Vec<EnqueueDownloadBatchItem>,
}

/// The batch the files go into.
pub enum Batching {
  /// none
  Single,
  /// one that exists
  Existing(Uuid),
  /// one made with them
  New(Box<NewBatch>),
}

/// What an enqueue did.
pub struct Enqueued {
  /// the downloads made
  pub enqueued: Vec<Transfer>,
  /// the files not enqueued, and why
  pub failed: Vec<EnqueueFailure>,
}

/// What the transaction wrote.
struct Written {
  inserted: Vec<Record>,
  superseded: Vec<Removed>,
  failed: Vec<EnqueueFailure>,
}

/// Refuses an enqueue without a user or files, with a file without a name or with a `.` or
/// `..` segment, or with a file named twice.
fn check(username: &str, files: &[FileRequest]) -> Result<(), ApiError> {
  if username.trim().is_empty() {
    return Err(ApiError::bad_request("Username is required"));
  }
  if files.is_empty() {
    return Err(ApiError::bad_request("At least one file is required"));
  }
  if files.iter().any(|f| f.filename.is_empty()) {
    return Err(ApiError::bad_request(
      "At least one filename is null or empty",
    ));
  }
  if files.iter().any(|f| traverses(&f.filename)) {
    return Err(ApiError::bad_request(
      "One or more files contains a dangerous path traversal segment",
    ));
  }
  let mut seen = HashSet::new();
  if !files.iter().all(|f| seen.insert(f.filename.as_str())) {
    return Err(ApiError::bad_request(
      "Two or more files in request are duplicated",
    ));
  }
  Ok(())
}

/// The answer to a failure reaching `username`.
fn unreachable(username: &str, error: ClientError) -> ApiError {
  match error {
    ClientError::UserOffline(_) | ClientError::UserNotFound(_) => ApiError::offline(username),
    ClientError::InvalidOperation(message) => ApiError::unavailable(Some(message)),
    other => ApiError::upstream(
      format!("{username} could not be reached"),
      Some(other.to_string()),
    ),
  }
}

impl Downloads {
  /// Enqueues `files` from `username` into `batching`. A user blocked from downloads through
  /// the API is a conflict; one offline or blocked by the configuration a 404. Files already
  /// downloading are listed as failed and the rest enqueued; a new batch is made with them,
  /// and a batch id already taken is a conflict.
  pub async fn enqueue(
    &self,
    username: &str,
    files: Vec<FileRequest>,
    batching: Batching,
  ) -> Result<Enqueued, ApiError> {
    check(username, &files)?;
    self.reachable(username).await?;
    let _turn = self.turn(username).await;
    let batch_id = match &batching {
      Batching::Single => None,
      Batching::Existing(id) => Some(*id),
      Batching::New(batch) => Some(batch.row.id),
    };
    let tracked: HashSet<String> = self
      .network
      .client()
      .downloads()
      .into_iter()
      .filter(|t| t.username == username)
      .map(|t| t.filename.as_str().to_string())
      .collect();
    let candidates: Vec<Record> = files
      .iter()
      .map(|file| Record::download(username, &file.filename, file.size, batch_id))
      .collect();
    let ids: Vec<Uuid> = candidates.iter().map(|record| record.id).collect();
    // the handles exist before the records can be seen, so a cancel always finds them
    {
      let mut in_flight = lock(&self.in_flight);
      for id in &ids {
        in_flight.insert(
          *id,
          InFlight {
            cancel: self.shutdown.child_token(),
            remove: false,
            reason: None,
          },
        );
      }
    }
    let name = username.to_string();
    let written = self
      .db
      .write(move |conn| Ok(write(conn, &name, candidates, batching, &tracked)))
      .await
      .map_err(ApiError::internal)
      .and_then(|written| {
        written.map_err(|e| match e {
          InsertError::Exists(id) => {
            ApiError::conflict(format!("A batch with ID {id} already exists"), None)
          }
          InsertError::Db(e) => ApiError::internal(e),
        })
      });
    let written = match written {
      Ok(written) => written,
      Err(e) => {
        let mut in_flight = lock(&self.in_flight);
        for id in &ids {
          in_flight.remove(id);
        }
        return Err(e);
      }
    };
    let inserted: HashSet<Uuid> = written.inserted.iter().map(|r| r.id).collect();
    {
      let mut in_flight = lock(&self.in_flight);
      for id in ids.iter().filter(|id| !inserted.contains(id)) {
        in_flight.remove(id);
      }
    }
    removal::push(&self.live, TransferDirection::Download, &written.superseded);
    if !written.inserted.is_empty() {
      self.users.touch(username, Touch::Transfer).await;
    }
    let gate = Arc::new(Semaphore::new(if files.len() <= ALL_AT_ONCE {
      files.len()
    } else {
      AT_ONCE
    }));
    let mut enqueued = Vec::new();
    for record in written.inserted {
      let transfer = record.transfer();
      self
        .live
        .publish(LiveFrame::TransfersAdded(Box::new(transfer.clone())));
      info!(id = %record.id, username, filename = record.filename, "download queued");
      let cancel = lock(&self.in_flight)
        .get(&record.id)
        .map(|entry| entry.cancel.clone())
        .expect("the handle of an inserted download is kept");
      self.spawn(record, gate.clone(), cancel);
      enqueued.push(transfer);
    }
    Ok(Enqueued {
      enqueued,
      failed: written.failed,
    })
  }

  /// Refuses a user blocked from downloads through the API, one offline, and one the
  /// configuration blocks; reaches the others with a message connection the downloads share.
  async fn reachable(&self, username: &str) -> Result<(), ApiError> {
    if self
      .users
      .blocked_through_api(username, BlockScope::Downloads)
    {
      debug!(
        username,
        "enqueue from a user blocked from downloads refused"
      );
      return Err(ApiError::conflict("user is blocked", None));
    }
    let client = self.network.client();
    let endpoint = client
      .get_user_endpoint(username)
      .await
      .map_err(|e| unreachable(username, e))?;
    if self
      .users
      .is_blocked(username, Some(endpoint.ip()), BlockScope::Downloads)
    {
      debug!(
        username,
        "enqueue from a blacklisted user refused as offline"
      );
      return Err(ApiError::offline(username));
    }
    client
      .connect_to_user(username, false)
      .await
      .map_err(|e| unreachable(username, e))
  }
}

/// Writes, in one transaction, the batch to make and every candidate record whose file is not
/// downloading already, superseding the user's earlier records of the same file.
fn write(
  conn: &mut rusqlite::Connection,
  username: &str,
  candidates: Vec<Record>,
  batching: Batching,
  tracked: &HashSet<String>,
) -> Result<Written, InsertError> {
  let mut tx = conn.transaction()?;
  if let Batching::New(batch) = &batching {
    batches::insert(&tx, &batch.row, &batch.files)?;
  }
  let running: HashSet<String> = {
    let mut statement = tx.prepare(&format!(
      "SELECT Filename FROM Transfers WHERE Username = ?1 AND Direction = ?2 AND Removed = 0 \
       AND {UNFINISHED}"
    ))?;
    let rows = statement.query_map(
      params![username, direction_text(TransferDirection::Download)],
      |row| row.get::<_, Option<String>>(0),
    )?;
    rows
      .filter_map(Result::transpose)
      .collect::<rusqlite::Result<_>>()?
  };
  let mut written = Written {
    inserted: Vec::new(),
    superseded: Vec::new(),
    failed: Vec::new(),
  };
  for record in candidates {
    if running.contains(&record.filename) {
      written.failed.push(EnqueueFailure {
        filename: record.filename,
        message: ALREADY_RUNNING.to_string(),
      });
      continue;
    }
    if tracked.contains(&record.filename) {
      warn!(
        username,
        filename = record.filename,
        "the client downloads a file the records do not; not enqueued again"
      );
      written.failed.push(EnqueueFailure {
        filename: record.filename,
        message: ALREADY_RUNNING.to_string(),
      });
      continue;
    }
    let mut file = tx.savepoint()?;
    match insert_superseding(&file, &record) {
      Ok(superseded) => {
        file.commit()?;
        written.superseded.extend(superseded);
        written.inserted.push(record);
      }
      Err(e) => {
        file.rollback()?;
        warn!(username, filename = record.filename, error = %e, "download not enqueued");
        written.failed.push(EnqueueFailure {
          filename: record.filename,
          message: format!("Error: {e}"),
        });
      }
    }
  }
  tx.commit()?;
  Ok(written)
}

/// Writes `record` and removes the earlier records of its file and peer.
fn insert_superseding(
  conn: &rusqlite::Connection,
  record: &Record,
) -> rusqlite::Result<Vec<Removed>> {
  store::insert(conn, record)?;
  removal::supersede(conn, record)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn file(name: &str) -> FileRequest {
    FileRequest {
      filename: name.to_string(),
      size: 1,
    }
  }

  #[test]
  fn an_enqueue_is_refused_for_what_it_cannot_download() {
    let message = |username: &str, files: &[FileRequest]| {
      check(username, files).unwrap_err().message().to_string()
    };
    assert!(check("alice", &[file("a\\b.flac"), file("file..ext")]).is_ok());
    assert_eq!(message(" ", &[file("a")]), "Username is required");
    assert_eq!(message("alice", &[]), "At least one file is required");
    assert_eq!(
      message("alice", &[file("")]),
      "At least one filename is null or empty"
    );
    assert_eq!(
      message("alice", &[file("a/../b")]),
      "One or more files contains a dangerous path traversal segment"
    );
    let same_name = [
      file("a\\b.flac"),
      FileRequest {
        filename: "a\\b.flac".to_string(),
        size: 2,
      },
    ];
    assert_eq!(
      message("alice", &same_name),
      "Two or more files in request are duplicated"
    );
  }
}
