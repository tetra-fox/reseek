//! The download calls of the API, answered by the engine, and the client's own records of its
//! transfers.

use api_types::Page;
use api_types::transfers::{
  BulkAction, BulkFailure, BulkRequest, BulkResult, ClientTransfers, DebugTransfers,
  EnqueueDownloadGroup, EnqueueDownloadJobRequest, EnqueueDownloadJobResponse,
  EnqueuedDownloadGroup, Transfer, TransferDirection, TransferListQuery, UserResponse,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tracing::{error, info};
use uuid::Uuid;

use api_types::jobs::JobStarter;

use super::{Downloads, FileRequest, Group, internal};
use crate::api::error::ApiError;
use crate::services::TransferService;
use crate::transfers::categories::UNFINISHED;
use crate::transfers::{ASKED_ORDER, COLUMNS, Record, listing, removal, store};

/// What a refused enqueue call is told when every one allowed at once runs already.
const BUSY: &str =
  "Too many download requests are being handled at once. Wait until one of them completes.";

/// One of the client's transfers as `GET /debug/transfers` shows it.
fn client_transfer(transfer: &soulseek_client::Transfer) -> serde_json::Value {
  let time = |at: Option<std::time::SystemTime>| at.map(chrono::DateTime::<Utc>::from);
  json!({
    "direction": api_types::transfers::TransferDirection::from(transfer.direction),
    "username": transfer.username,
    "filename": transfer.filename.to_string(),
    "token": transfer.token,
    "remoteToken": transfer.remote_token,
    "state": transfer.state.to_string(),
    "size": transfer.size,
    "startOffset": transfer.start_offset,
    "bytesTransferred": transfer.bytes_transferred,
    "averageSpeed": transfer.average_speed,
    "startTime": time(transfer.start_time),
    "endTime": time(transfer.end_time),
    "ipEndPoint": transfer.ip_end_point.map(|e| e.to_string()),
    "exception": transfer.error.as_ref().map(ToString::to_string),
  })
}

impl Downloads {
  /// Takes one of the enqueue calls that may run at once; 429 when all of them run.
  fn enqueue_call(&self) -> Result<tokio::sync::SemaphorePermit<'_>, ApiError> {
    self
      .enqueue_calls
      .try_acquire()
      .map_err(|_| ApiError::too_many(BUSY))
  }

  /// Enqueues the completed downloads of `records` again as one new job started by the user,
  /// each with its destination and search; returns the job, none when nothing could be retried,
  /// and says for each download whether it was.
  async fn retry(&self, records: Vec<Record>) -> (Option<Uuid>, Vec<Result<Uuid, BulkFailure>>) {
    let (groups, refused) = retry_groups(records);
    let mut results: Vec<Result<Uuid, BulkFailure>> = refused.into_iter().map(Err).collect();
    if groups.is_empty() {
      return (None, results);
    }
    let (asked, retried): (Vec<_>, Vec<_>) = groups
      .into_iter()
      .map(|retried| (retried.group, retried.records))
      .unzip();
    let (job, outcomes) = self.enqueue_new(JobStarter::you(), asked, false).await;
    for (records, outcome) in retried.into_iter().zip(outcomes) {
      match outcome {
        Ok(enqueued) => {
          for record in records {
            match enqueued
              .failed
              .iter()
              .find(|f| f.filename == record.filename)
            {
              Some(failed) => results.push(Err(BulkFailure {
                id: record.id,
                error: failed.message.clone(),
              })),
              None => results.push(Ok(record.id)),
            }
          }
        }
        Err(e) => results.extend(records.into_iter().map(|record| {
          Err(BulkFailure {
            id: record.id,
            error: e.message().to_string(),
          })
        })),
      }
    }
    (Some(job), results)
  }

  /// Queues again, as a new job started by the user, the downloads of the job `job` that failed
  /// or were canceled; returns the new job. 409 when none did.
  pub async fn run_again(&self, job: Uuid) -> Result<Uuid, ApiError> {
    let records = self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(&format!(
          "SELECT {COLUMNS} FROM Transfers WHERE JobId = ?1 AND Removed = 0 \
           AND NOT {UNFINISHED} AND State <> 48 ORDER BY {ASKED_ORDER}"
        ))?;
        statement
          .query_map([job.to_string()], Record::read)?
          .collect::<rusqlite::Result<Vec<_>>>()
      })
      .await
      .map_err(internal)?;
    if records.is_empty() {
      return Err(ApiError::conflict("No download of the job failed", None));
    }
    let (new, results) = self.retry(records).await;
    let refused = results.iter().filter(|result| result.is_err()).count();
    info!(%job, ?new, refused, "download job run again");
    new.ok_or_else(|| ApiError::conflict("No download of the job could be queued again", None))
  }
}

/// Downloads retried together: a group of one user, destination and search, and the records
/// whose files it holds.
struct Retried {
  group: Group,
  records: Vec<Record>,
}

/// The downloads of `records` to retry, in groups, and why the others are not: one not
/// completed, and a second of a file of the same user.
fn retry_groups(records: Vec<Record>) -> (Vec<Retried>, Vec<BulkFailure>) {
  let mut groups: Vec<Retried> = Vec::new();
  let mut refused = Vec::new();
  for record in records {
    if !record.is_completed() {
      refused.push(BulkFailure {
        id: record.id,
        error: "the download has not completed".to_string(),
      });
      continue;
    }
    let again = groups.iter().any(|retried| {
      retried.group.username == record.username
        && retried
          .group
          .files
          .iter()
          .any(|f| f.filename == record.filename)
    });
    if again {
      refused.push(BulkFailure {
        id: record.id,
        error: "another download of the same file is retried".to_string(),
      });
      continue;
    }
    match groups
      .iter_mut()
      .find(|retried| retried.group.holds(&record))
    {
      Some(retried) => {
        retried.group.files.push(FileRequest::of(&record));
        retried.records.push(record);
      }
      None => groups.push(Retried {
        group: Group::of(&record),
        records: vec![record],
      }),
    }
  }
  (groups, refused)
}

/// The group of the engine a group of the API asks for.
fn group(request: EnqueueDownloadGroup) -> Group {
  Group {
    username: request.username,
    files: request
      .files
      .into_iter()
      .map(|f| FileRequest {
        filename: f.filename,
        size: f.size,
      })
      .collect(),
    destination: request.destination,
    search_id: request.search_id,
  }
}

#[async_trait]
impl TransferService for Downloads {
  async fn downloads(&self, include_removed: bool) -> Result<Vec<UserResponse>, ApiError> {
    listing::grouped(&self.db, TransferDirection::Download, include_removed, None)
      .await
      .map_err(internal)
  }

  async fn download_list(&self, query: TransferListQuery) -> Result<Page<Transfer>, ApiError> {
    listing::page(&self.db, TransferDirection::Download, query)
      .await
      .map_err(internal)
  }

  async fn user_downloads(&self, username: &str) -> Result<UserResponse, ApiError> {
    listing::grouped(
      &self.db,
      TransferDirection::Download,
      false,
      Some(username.to_string()),
    )
    .await
    .map_err(internal)?
    .pop()
    .ok_or_else(|| ApiError::not_found(format!("No downloads from {username}")))
  }

  async fn download(
    &self,
    username: &str,
    id: Uuid,
    include_history: bool,
  ) -> Result<Transfer, ApiError> {
    listing::one(
      &self.db,
      TransferDirection::Download,
      username,
      id,
      include_history,
    )
    .await
    .map_err(internal)?
    .ok_or_else(|| ApiError::not_found(format!("No download {id} from {username}")))
  }

  async fn place_in_queue(&self, username: &str, id: Uuid) -> Result<u32, ApiError> {
    Downloads::place_in_queue(self, username, id).await
  }

  async fn enqueue_job(
    &self,
    request: EnqueueDownloadJobRequest,
  ) -> Result<EnqueueDownloadJobResponse, ApiError> {
    let _call = self.enqueue_call()?;
    let groups: Vec<Group> = request.groups.into_iter().map(group).collect();
    let usernames: Vec<String> = groups.iter().map(|g| g.username.clone()).collect();
    let (job, outcomes) = self.enqueue_new(JobStarter::you(), groups, true).await;
    let groups = usernames
      .into_iter()
      .zip(outcomes)
      .map(|(username, outcome)| match outcome {
        Ok(enqueued) => EnqueuedDownloadGroup {
          username,
          enqueued: enqueued.enqueued,
          failures: enqueued.failed,
          error: None,
        },
        Err(e) => {
          match e.reason() {
            Some(reason) => error!(username, %job, reason, "download job's group failed"),
            None => info!(
              username,
              %job,
              error = e.message(),
              "download job's group refused"
            ),
          }
          EnqueuedDownloadGroup {
            username,
            enqueued: Vec::new(),
            failures: Vec::new(),
            error: Some(e.into_body()),
          }
        }
      })
      .collect();
    Ok(EnqueueDownloadJobResponse { id: job, groups })
  }

  async fn cancel_download(&self, username: &str, id: Uuid, remove: bool) -> Result<(), ApiError> {
    self.cancel(username, id, remove).await
  }

  async fn remove_completed_downloads(&self) -> Result<(), ApiError> {
    removal::remove_completed(&self.db, &self.live, TransferDirection::Download)
      .await
      .map(drop)
      .map_err(internal)
  }

  async fn bulk_downloads(&self, request: BulkRequest) -> Result<BulkResult, ApiError> {
    let ids = request.ids.clone();
    let found = self
      .db
      .read(move |conn| {
        ids
          .into_iter()
          .map(|id| store::get(conn, id).map(|record| (id, record)))
          .collect::<rusqlite::Result<Vec<_>>>()
      })
      .await
      .map_err(internal)?;
    let mut result = BulkResult {
      ok: Vec::new(),
      failed: Vec::new(),
    };
    let mut records = Vec::new();
    for (id, record) in found {
      match record.filter(|r| r.direction == TransferDirection::Download) {
        Some(record) => records.push(record),
        None => result.failed.push(BulkFailure {
          id,
          error: format!("no download {id}"),
        }),
      }
    }
    let outcomes: Vec<Result<Uuid, BulkFailure>> = match request.action {
      BulkAction::Retry => self.retry(records).await.1,
      BulkAction::Pause => self.pause(records.iter().map(|r| r.id).collect()).await,
      BulkAction::Resume => {
        self
          .resume_paused(records.iter().map(|r| r.id).collect())
          .await
      }
      BulkAction::Cancel | BulkAction::Remove => {
        let remove = request.action == BulkAction::Remove;
        let mut outcomes = Vec::new();
        for record in records {
          if !remove && record.is_completed() {
            outcomes.push(Err(BulkFailure {
              id: record.id,
              error: "the download has completed".to_string(),
            }));
            continue;
          }
          outcomes.push(
            self
              .cancel(&record.username, record.id, remove)
              .await
              .map(|()| record.id)
              .map_err(|e| BulkFailure {
                id: record.id,
                error: e.message().to_string(),
              }),
          );
        }
        outcomes
      }
    };
    for outcome in outcomes {
      match outcome {
        Ok(id) => result.ok.push(id),
        Err(failure) => result.failed.push(failure),
      }
    }
    Ok(result)
  }

  async fn client_transfers(&self) -> Result<DebugTransfers, ApiError> {
    let client = self.network.client();
    Ok(DebugTransfers {
      soulseek: ClientTransfers {
        uploads: client.uploads().iter().map(client_transfer).collect(),
        downloads: client.downloads().iter().map(client_transfer).collect(),
      },
    })
  }
}

#[cfg(test)]
mod tests {
  use api_types::transfers::TransferStates;

  use super::*;

  #[test]
  fn a_retry_keeps_each_downloads_destination_and_search() {
    let search = Uuid::new_v4();
    let ended = |username: &str, filename: &str, destination: Option<&str>| Record {
      state: TransferStates::Completed | TransferStates::Errored,
      destination: destination.map(str::to_string),
      search_id: Some(search),
      ..Record::download(username, filename, 1)
    };
    let one = ended("alice", "a\\1\\1.flac", Some("Album/Disc 1"));
    let two = ended("alice", "a\\2\\1.flac", Some("Album/Disc 2"));
    let three = ended("alice", "a\\1\\2.flac", Some("Album/Disc 1"));
    let again = ended("alice", "a\\1\\1.flac", None);
    let bob = ended("bob", "a\\1\\1.flac", Some("Album/Disc 1"));
    let running = Record::download("alice", "a\\3.flac", 1);
    let (groups, refused) = retry_groups(vec![
      one.clone(),
      two.clone(),
      running.clone(),
      three.clone(),
      again.clone(),
      bob.clone(),
    ]);
    let ids = |records: &[Record]| records.iter().map(|r| r.id).collect::<Vec<_>>();
    let shape: Vec<(Group, Vec<Uuid>)> = groups
      .into_iter()
      .map(|retried| (retried.group, ids(&retried.records)))
      .collect();
    let group = |records: &[&Record]| Group {
      files: records.iter().map(|r| FileRequest::of(r)).collect(),
      ..Group::of(records[0])
    };
    assert_eq!(
      shape,
      [
        (group(&[&one, &three]), vec![one.id, three.id]),
        (group(&[&two]), vec![two.id]),
        (group(&[&bob]), vec![bob.id]),
      ]
    );
    assert_eq!(shape[0].0.destination.as_deref(), Some("Album/Disc 1"));
    assert_eq!(shape[0].0.search_id, Some(search));
    let refused: Vec<(Uuid, String)> = refused.into_iter().map(|f| (f.id, f.error)).collect();
    assert_eq!(
      refused,
      [
        (running.id, "the download has not completed".to_string()),
        (
          again.id,
          "another download of the same file is retried".to_string()
        ),
      ]
    );
  }
}
