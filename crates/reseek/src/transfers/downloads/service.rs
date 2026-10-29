//! The download calls of the API, answered by the engine, and the client's own records of its
//! transfers.

use std::collections::BTreeMap;

use api_types::Page;
use api_types::transfers::{
  Batch, BulkAction, BulkFailure, BulkRequest, BulkResult, ClientTransfers, DebugTransfers,
  EnqueueDownloadBatchRequest, EnqueueDownloadBatchResponse, QueueDownloadRequest,
  QueueDownloadResponse, Transfer, TransferDirection, TransferListQuery, UserResponse,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use super::batches::{self, BatchRow};
use super::{Batching, Downloads, FileRequest, NewBatch, internal};
use crate::api::error::ApiError;
use crate::services::TransferService;
use crate::transfers::{Record, listing, removal, store};

/// What a refused enqueue call is told when every one allowed at once runs already.
const BUSY: &str =
  "Only one concurrent operation is permitted. Wait until the previous request completes";

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

  /// Enqueues the completed downloads of `records` again, each in its batch, and says for each
  /// whether it was.
  async fn retry(&self, records: Vec<Record>) -> Vec<Result<Uuid, BulkFailure>> {
    let mut results = Vec::new();
    let mut groups: BTreeMap<(String, Option<Uuid>), Vec<Record>> = BTreeMap::new();
    for record in records {
      if !record.is_completed() {
        results.push(Err(BulkFailure {
          id: record.id,
          error: "the download has not completed".to_string(),
        }));
        continue;
      }
      let group = groups
        .entry((record.username.clone(), record.batch_id))
        .or_default();
      if group.iter().any(|r| r.filename == record.filename) {
        results.push(Err(BulkFailure {
          id: record.id,
          error: "another download of the same file is retried".to_string(),
        }));
        continue;
      }
      group.push(record);
    }
    for ((username, batch), records) in groups {
      let files = records
        .iter()
        .map(|r| FileRequest {
          filename: r.filename.clone(),
          size: r.size,
        })
        .collect();
      let batching = batch.map_or(Batching::Single, Batching::Existing);
      match self.enqueue(&username, files, batching).await {
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
    results
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
    .ok_or_else(|| ApiError::not_found(format!("no downloads from {username}")))
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
    .ok_or_else(|| ApiError::not_found(format!("no download {id} from {username}")))
  }

  async fn place_in_queue(&self, username: &str, id: Uuid) -> Result<u32, ApiError> {
    Downloads::place_in_queue(self, username, id).await
  }

  async fn enqueue(
    &self,
    username: &str,
    files: Vec<QueueDownloadRequest>,
  ) -> Result<QueueDownloadResponse, ApiError> {
    let _call = self.enqueue_call()?;
    let files = files
      .into_iter()
      .map(|f| FileRequest {
        filename: f.filename,
        size: f.size,
      })
      .collect();
    let enqueued = Downloads::enqueue(self, username, files, Batching::Single).await?;
    Ok(QueueDownloadResponse {
      enqueued: enqueued.enqueued,
      failed: enqueued.failed,
    })
  }

  async fn enqueue_batch(
    &self,
    request: EnqueueDownloadBatchRequest,
  ) -> Result<EnqueueDownloadBatchResponse, ApiError> {
    let _call = self.enqueue_call()?;
    let id = request.id.unwrap_or_else(Uuid::new_v4);
    let files = request
      .files
      .iter()
      .map(|f| FileRequest {
        filename: f.filename.clone(),
        size: f.size,
      })
      .collect();
    let batch = NewBatch {
      row: BatchRow {
        id,
        search_id: request.search_id,
        username: request.username.clone(),
        created_at: Utc::now(),
        options: request.options,
      },
      files: request.files,
    };
    let enqueued = Downloads::enqueue(
      self,
      &request.username,
      files,
      Batching::New(Box::new(batch)),
    )
    .await?;
    let batch = batches::read(&self.db, id, false)
      .await
      .map_err(internal)?
      .ok_or_else(|| ApiError::internal(format!("the batch {id} just made is gone")))?;
    Ok(EnqueueDownloadBatchResponse {
      batch,
      failures: enqueued.failed,
    })
  }

  async fn batch(&self, id: Uuid, include_removed: bool) -> Result<Batch, ApiError> {
    batches::read(&self.db, id, include_removed)
      .await
      .map_err(internal)?
      .ok_or_else(|| ApiError::not_found(format!("no batch {id}")))
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
      BulkAction::Retry => self.retry(records).await,
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
