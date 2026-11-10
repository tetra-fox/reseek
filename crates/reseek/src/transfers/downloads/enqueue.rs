//! Enqueueing downloads, as a job of one or more groups of files from one user each: the job is
//! opened, then for each group the request and the peer are checked (the peer only when a file
//! is not whole in the incomplete directory already) and, in the user's turn, a
//! record is written for every file not downloading already, in one transaction, carrying the
//! job, the group's destination and its search, and a task is started for each; then the job is
//! closed. A record for a file that finished earlier is superseded by the new one.

use std::collections::HashSet;

use api_types::live::LiveFrame;
use api_types::transfers::{EnqueueFailure, Transfer, TransferDirection};
use api_types::users::blocking::BlockScope;
use regex::Regex;
use rusqlite::params;
use soulseek_client::ClientError;
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

/// How many of `files` files of one request ask their peer at once.
pub(super) fn gate_for(files: usize) -> usize {
  if files <= ALL_AT_ONCE { files } else { AT_ONCE }
}

/// Why a file matching a download filter was skipped.
const FILTERED: &str = "Skipped: Filtered";

/// What scripts read as a file already downloading.
const ALREADY_RUNNING: &str = "Skipped: Already in progress";

/// One file to download.
#[derive(Clone, Debug, PartialEq)]
pub struct FileRequest {
  /// the remote filename exactly as the peer shares it
  pub filename: String,
  /// its size in bytes, 0 when not known
  pub size: u64,
  /// the place in the local queue a download of it held before a restart, which it keeps;
  /// none for the end of the queue
  pub position: Option<f64>,
}

/// Files from one user to enqueue together, one group of a job, with where they go and what
/// found them, which each of their downloads keeps.
#[derive(Debug, PartialEq)]
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
  /// The file of the download `record`, asked for again at the end of the queue.
  pub fn of(record: &Record) -> Self {
    Self {
      filename: record.filename.clone(),
      size: record.size,
      position: None,
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
  /// came of it. With `filtered`, files matching `transfers.download.filters` are skipped and
  /// listed as failed. A user blocked from downloads through the API is a conflict; one offline or
  /// blocked by the configuration a 404. Files already downloading are listed as failed and the
  /// rest enqueued. The job is open until every group was tried, so it is not over before all
  /// its files are in.
  pub async fn enqueue(
    &self,
    job: Uuid,
    groups: Vec<Group>,
    filtered: bool,
  ) -> Vec<Result<Enqueued, ApiError>> {
    let filters: Vec<Regex> = if filtered {
      self
        .config
        .current()
        .values
        .transfers
        .download
        .filters
        .iter()
        // validation refuses a filter that does not compile
        .filter_map(|pattern| reseek_shares::patterns::compile(pattern, false).ok())
        .collect()
    } else {
      Vec::new()
    };
    // the job runs on a task of its own, so a caller that stops waiting, such as a request
    // whose connection drops, leaves no record without its task and no job open
    let this = self.this();
    let count = groups.len();
    let ran = self.tasks.spawn(
      async move {
        this.open_job(job);
        let mut results = Vec::with_capacity(groups.len());
        for group in groups {
          results.push(this.enqueue_group(job, group, &filters).await);
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

  /// Enqueues one group of the job `job`, skipping the files that match `filters`.
  async fn enqueue_group(
    &self,
    job: Uuid,
    mut group: Group,
    filters: &[Regex],
  ) -> Result<Enqueued, ApiError> {
    check(&group.username, &group.files)?;
    let (skipped, kept): (Vec<FileRequest>, Vec<FileRequest>) = group
      .files
      .into_iter()
      .partition(|file| filters.iter().any(|filter| filter.is_match(&file.filename)));
    let skipped: Vec<EnqueueFailure> = skipped
      .into_iter()
      .map(|file| {
        debug!(
          username = group.username,
          filename = file.filename,
          "download filtered"
        );
        EnqueueFailure {
          filename: file.filename,
          message: FILTERED.to_string(),
        }
      })
      .collect();
    if kept.is_empty() {
      return Ok(Enqueued {
        enqueued: Vec::new(),
        failed: skipped,
      });
    }
    group.files = kept;
    let mut whole = true;
    for file in &group.files {
      whole = whole && self.whole_here(&group.username, file).await;
    }
    self.admit(&group.username, whole).await?;
    let mut enqueued = self.make(job, group).await?;
    enqueued.failed.extend(skipped);
    Ok(enqueued)
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
        queue_position: file.position,
        ..Record::download(username, &file.filename, file.size)
      })
      .collect();
    let ids: Vec<Uuid> = candidates.iter().map(|record| record.id).collect();
    // the handles exist before the records can be seen, so a cancel always finds them
    {
      let mut in_flight = lock(&self.in_flight);
      for id in &ids {
        in_flight.insert(*id, InFlight::new(self.shutdown.child_token()));
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
    let request = (self.starts.request(), gate_for(files.len()));
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
      let position = record
        .queue_position
        .expect("the write places every download it inserts");
      self.spawn(record, position, request, cancel);
      enqueued.push(transfer);
    }
    Ok(Enqueued {
      enqueued,
      failed: written.failed,
    })
  }

  /// Refuses a user blocked from downloads through the API, one offline, and one the
  /// configuration blocks; reaches the others with a message connection the downloads share.
  /// For files `whole` here already the peer is not needed, so it is neither asked nor reached,
  /// and the configuration's blocks are checked by the name and the address last known.
  async fn admit(&self, username: &str, whole: bool) -> Result<(), ApiError> {
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
    let ip = if whole {
      None
    } else {
      let endpoint = client
        .get_user_endpoint(username)
        .await
        .map_err(|e| unreachable(username, e))?;
      Some(endpoint.ip())
    };
    if self.users.is_blocked(username, ip, BlockScope::Downloads) {
      debug!(
        username,
        "enqueue from a blacklisted user refused as offline"
      );
      return Err(ApiError::offline(username));
    }
    if whole {
      debug!(
        username,
        "every file is here already; the peer is not reached"
      );
      return Ok(());
    }
    client
      .connect_to_user(username, false)
      .await
      .map_err(|e| unreachable(username, e))
  }
}

/// Writes, in one transaction, every candidate record whose file is not downloading already,
/// superseding the user's earlier records of the same file. A candidate without a place in the
/// local queue is placed after every download.
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
  let mut next = store::next_position(&tx)?;
  for mut record in candidates {
    if record.queue_position.is_none() {
      record.queue_position = Some(next);
      next += 1.0;
    }
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
      position: None,
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
        position: None,
      },
    ];
    assert_eq!(
      message("alice", &same_name),
      "Two or more files in the request are duplicated"
    );
  }
}
