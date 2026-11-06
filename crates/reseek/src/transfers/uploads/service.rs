//! The upload calls of the API, answered by the engine: the listings, one upload with what it
//! waits for, bumping it, canceling and removing.

use api_types::Page;
use api_types::live::LiveFrame;
use api_types::transfers::{
  BulkAction, BulkFailure, BulkRequest, BulkResult, Transfer, TransferDirection, TransferListQuery,
  UploadQueueStatus, UploadsQuery, UserResponse,
};
use async_trait::async_trait;
use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use super::Uploads;
use super::limits;
use super::queue::Bumped;
use crate::api::error::ApiError;
use crate::services::TransferService;
use crate::transfers::listing::{self, Range};
use crate::transfers::{removal, store};

const UPLOAD: TransferDirection = TransferDirection::Upload;

fn internal(error: impl std::fmt::Display) -> ApiError {
  ApiError::internal(error)
}

#[async_trait]
impl TransferService for Uploads {
  async fn uploads(&self, query: UploadsQuery) -> Result<Vec<UserResponse>, ApiError> {
    let range = Range {
      include_removed: query.include_removed,
      since: query.since,
      until: query.until,
      limit: query.limit,
    };
    listing::grouped_in(&self.db, UPLOAD, range)
      .await
      .map_err(internal)
  }

  async fn upload_list(&self, query: TransferListQuery) -> Result<Page<Transfer>, ApiError> {
    listing::page(&self.db, UPLOAD, query)
      .await
      .map_err(internal)
  }

  async fn user_uploads(&self, username: &str) -> Result<UserResponse, ApiError> {
    let listed = listing::grouped(&self.db, UPLOAD, false, Some(username.to_string()))
      .await
      .map_err(internal)?;
    Ok(listed.into_iter().next().unwrap_or_else(|| UserResponse {
      username: username.to_string(),
      directories: Vec::new(),
    }))
  }

  async fn upload(
    &self,
    username: &str,
    id: Uuid,
    include_history: bool,
  ) -> Result<Transfer, ApiError> {
    listing::one(&self.db, UPLOAD, username, id, include_history)
      .await
      .map_err(internal)?
      .ok_or_else(|| ApiError::not_found(format!("No upload {id} to {username}")))
  }

  async fn cancel_upload(&self, username: &str, id: Uuid, remove: bool) -> Result<(), ApiError> {
    self.cancel(username, id, remove).await
  }

  async fn remove_completed_uploads(&self) -> Result<(), ApiError> {
    removal::remove_completed(&self.db, &self.live, UPLOAD)
      .await
      .map(drop)
      .map_err(internal)
  }

  async fn bulk_uploads(&self, request: BulkRequest) -> Result<BulkResult, ApiError> {
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
    let failed = |id: Uuid, error: &str| BulkFailure {
      id,
      error: error.to_string(),
    };
    for (id, record) in found {
      let Some(record) = record.filter(|r| r.direction == UPLOAD) else {
        result.failed.push(failed(id, &format!("no upload {id}")));
        continue;
      };
      let outcome = match request.action {
        BulkAction::Retry => Err(failed(
          id,
          "an upload is not retried; its peer asks for the file again",
        )),
        BulkAction::Pause | BulkAction::Resume => {
          Err(failed(id, "an upload is not paused; cancel it instead"))
        }
        BulkAction::Cancel if record.is_completed() => Err(failed(id, "the upload has completed")),
        BulkAction::Cancel | BulkAction::Remove => {
          let remove = request.action == BulkAction::Remove;
          self
            .cancel(&record.username, id, remove)
            .await
            .map_err(|e| failed(id, e.message()))
        }
      };
      match outcome {
        Ok(()) => result.ok.push(id),
        Err(failure) => result.failed.push(failure),
      }
    }
    Ok(result)
  }

  async fn upload_queue_status(
    &self,
    username: &str,
    id: Uuid,
  ) -> Result<UploadQueueStatus, ApiError> {
    self.record_of(username, id).await?;
    let standing = self
      .queue
      .standing(id)
      .unwrap_or_else(|| self.queue.overview(username));
    let now = Utc::now();
    let name = username.to_string();
    let figures = self
      .db
      .read(move |conn| limits::statistics(conn, &name, now))
      .await
      .map_err(internal)?;
    let options = self.config.current();
    Ok(UploadQueueStatus {
      position: standing.position,
      limits: limits::usage(&options.values, &standing.group, &figures),
      group: standing.group,
      group_slots_used: standing.group_slots_used,
      group_slots: standing.group_slots,
      global_slots_used: standing.global_slots_used,
      global_slots: standing.global_slots,
      user_active: standing.user_active,
    })
  }

  async fn bump(&self, username: &str, id: Uuid) -> Result<(), ApiError> {
    let record = self.record_of(username, id).await?;
    match self.queue.bump(id) {
      Bumped::Waiting => {
        info!(username, %id, "upload put in front of its group");
        self
          .live
          .publish(LiveFrame::TransfersUpdated(Box::new(record.transfer())));
        Ok(())
      }
      Bumped::NotWaiting => Err(ApiError::conflict(
        "The upload is not waiting in the queue",
        None,
      )),
    }
  }
}
