//! Enqueueing downloads, as a job of one or more groups of files from one user each: the job is
//! opened, then for each group the request and the peer are checked and, in the user's turn, a
//! record is written for every file not downloading already, in one transaction, carrying the
//! job, the group's destination and its search, and a task is started for each; then the job is
//! closed. A record for a file that finished earlier is superseded by the new one.

use std::collections::HashSet;
use std::sync::Arc;

use api_types::live::LiveFrame;
use api_types::transfers::{EnqueueFailure, Transfer, TransferDirection};
use api_types::users::blocking::BlockScope;
use rusqlite::params;
use soulseek_client::ClientError;
use tokio::sync::Semaphore;
use tracing::{Instrument, debug, info, warn};
use uuid::Uuid;

use super::{Downloads, InFlight};
use crate::api::error::ApiError;
use crate::safety::traverses;
use crate::sync::lock;
use crate::transfers::categories::UNFINISHED;
use crate::transfers::removal::{self, Removed};
use crate::transfers::{Record, direction_text, store};
use crate::users::Touch;

/// Up to this many files, every one of a request asks its peer at once; more than this,
/// [`AT_ONCE`] at a time, since some peers drop connections asked for dozens of files at once.
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

/// Files from one user to enqueue together, one group of a job, with where they go and what
/// found them, which each of their downloads keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
  /// the user the files come from
  pub username: String,
  /// the files
  pub files: Vec<FileRequest>,
  /// the relative directory under the downloads directory the files go to, in place of the one
  /// the download pattern gives
  pub destination: Option<String>,
  /// the search the files were found by
  pub search_id: Option<Uuid>,
}

impl FileRequest {
  /// The file of the download `record`.
  pub fn of(record: &Record) -> Self {
    Self {
      filename: record.filename.clone(),
      size: record.size,
    }
  }
}

impl Group {
  /// A group holding the file of the download `record`, with its user, destination and search.
  pub fn of(record: &Record) -> Self {
    Self {
      username: record.username.clone(),
      files: vec![FileRequest::of(record)],
      destination: record.destination.clone(),
      search_id: record.search_id,
    }
  }

  /// Whether the file of `record` belongs in this group: it comes from the same user, goes to
  /// the same destination and was found by the same search.
  pub fn holds(&self, record: &Record) -> bool {
    self.username == record.username
      && self.destination == record.destination
      && self.search_id == record.search_id
  }
}

/// What an enqueue of a group did.
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
      "One or more files contain a dangerous path traversal segment",
    ));
  }
  let mut seen = HashSet::new();
  if !files.iter().all(|f| seen.insert(f.filename.as_str())) {
    return Err(ApiError::bad_request(
      "Two or more files in the request are duplicated",
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
      format!("Could not reach {username}"),
      Some(other.to_string()),
    ),
  }
}

impl Downloads {
  /// Enqueues `groups` as the job `job`, each group as its own enqueue, and says for each what
  /// came of it. A user blocked from downloads through the API is a conflict; one offline or
  /// blocked by the configuration a 404. Files already downloading are listed as failed and the
  /// rest enqueued. The job is open until every group was tried, so it is not over before all
  /// its files are in.
  pub async fn enqueue(&self, job: Uuid, groups: Vec<Group>) -> Vec<Result<Enqueued, ApiError>> {
    // the job runs on a task of its own, so a caller that stops waiting, such as a request
    // whose connection drops, leaves no record without its task and no job open
    let this = self.this();
    let count = groups.len();
    let ran = self.tasks.spawn(
      async move {
        this.open_job(job);
        let mut results = Vec::with_capacity(groups.len());
        for group in groups {
          results.push(this.enqueue_group(job, group).await);
        }
        this.close_job(job).await;
        results
      }
      .in_current_span(),
    );
    match ran.await {
      Ok(results) => results,
      Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
      Err(e) => {
        let message = e.to_string();
        (0..count)
          .map(|_| Err(ApiError::internal(&message)))
          .collect()
      }
    }
  }

  /// Enqueues one group of the job `job`.
  async fn enqueue_group(&self, job: Uuid, group: Group) -> Result<Enqueued, ApiError> {
    check(&group.username, &group.files)?;
    self.reachable(&group.username).await?;
    self.make(job, group).await
  }

  /// In the turn of the user of `group`, writes a record in the job `job` for every file of
  /// `group` not downloading already, and starts a task for each.
  async fn make(&self, job: Uuid, group: Group) -> Result<Enqueued, ApiError> {
    let Group {
      username,
      files,
      destination,
      search_id,
    } = group;
    let username = username.as_str();
    let _turn = self.turn(username).await;
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
      .map(|file| Record {
        job_id: Some(job),
        destination: destination.clone(),
        search_id,
        ..Record::download(username, &file.filename, file.size)
      })
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
      .write(move |conn| write(conn, &name, candidates, &tracked))
      .await
      .map_err(ApiError::internal);
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
      return Err(ApiError::conflict("The user is blocked", None));
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

/// Writes, in one transaction, every candidate record whose file is not downloading already,
/// superseding the user's earlier records of the same file.
fn write(
  conn: &mut rusqlite::Connection,
  username: &str,
  candidates: Vec<Record>,
  tracked: &HashSet<String>,
) -> rusqlite::Result<Written> {
  let mut tx = conn.transaction()?;
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
      "One or more files contain a dangerous path traversal segment"
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
      "Two or more files in the request are duplicated"
    );
  }
}
