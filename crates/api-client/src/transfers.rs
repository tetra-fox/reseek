//! The transfer calls: downloads and uploads.

use api_types::Page;
use api_types::transfers::{
  Batch, BulkRequest, BulkResult, CancelQuery, EnqueueDownloadBatchRequest,
  EnqueueDownloadBatchResponse, IncludeRemovedQuery, QueueDownloadRequest, QueueDownloadResponse,
  Transfer, TransferDirection, TransferListQuery, TransferQuery, UploadQueueStatus, UploadsQuery,
  UserResponse,
};
use uuid::Uuid;

use super::{Client, Result, Route, done, json};

/// The route of a direction's transfers.
fn transfers(direction: TransferDirection) -> Route {
  match direction {
    TransferDirection::Download => Route::new("transfers/downloads"),
    TransferDirection::Upload => Route::new("transfers/uploads"),
  }
}

/// The route of one transfer.
fn transfer(direction: TransferDirection, username: &str, id: Uuid) -> Route {
  transfers(direction)
    .segment(username)
    .segment(&id.to_string())
}

impl Client {
  /// `GET /transfers/downloads`: every download, grouped.
  pub async fn downloads(&self, include_removed: bool) -> Result<Vec<UserResponse>> {
    let route =
      transfers(TransferDirection::Download).query(&IncludeRemovedQuery { include_removed })?;
    json(self.get(route)).await
  }

  /// `GET /transfers/uploads`: every upload, grouped.
  pub async fn uploads(&self, query: &UploadsQuery) -> Result<Vec<UserResponse>> {
    json(self.get(transfers(TransferDirection::Upload).query(query)?)).await
  }

  /// `GET /transfers/{downloads,uploads}/list`: a page of one direction's transfers.
  pub async fn transfer_list(
    &self,
    direction: TransferDirection,
    query: &TransferListQuery,
  ) -> Result<Page<Transfer>> {
    json(self.get(transfers(direction).then("list").query(query)?)).await
  }

  /// `GET /transfers/{downloads,uploads}/{username}`: one user's transfers, grouped.
  pub async fn user_transfers(
    &self,
    direction: TransferDirection,
    username: &str,
  ) -> Result<UserResponse> {
    json(self.get(transfers(direction).segment(username))).await
  }

  /// `GET /transfers/{downloads,uploads}/{username}/{id}`: one transfer.
  pub async fn transfer(
    &self,
    direction: TransferDirection,
    username: &str,
    id: Uuid,
    include_history: bool,
  ) -> Result<Transfer> {
    let route = transfer(direction, username, id).query(&TransferQuery { include_history })?;
    json(self.get(route)).await
  }

  /// `DELETE /transfers/{downloads,uploads}/{username}/{id}`: cancels a transfer, and removes
  /// it with `remove`.
  pub async fn cancel_transfer(
    &self,
    direction: TransferDirection,
    username: &str,
    id: Uuid,
    remove: bool,
  ) -> Result<()> {
    let route = transfer(direction, username, id).query(&CancelQuery { remove })?;
    done(self.delete(route)).await
  }

  /// `DELETE /transfers/{downloads,uploads}/all/completed`: removes every completed transfer
  /// of a direction.
  pub async fn remove_completed(&self, direction: TransferDirection) -> Result<()> {
    done(self.delete(transfers(direction).then("all/completed"))).await
  }

  /// `POST /transfers/{downloads,uploads}/bulk`: one action on several transfers.
  pub async fn bulk(
    &self,
    direction: TransferDirection,
    request: &BulkRequest,
  ) -> Result<BulkResult> {
    json(self.post(transfers(direction).then("bulk")).json(request)).await
  }

  /// `GET /transfers/downloads/{username}/{id}/position`: a queued download's place in the
  /// peer's queue.
  pub async fn place_in_queue(&self, username: &str, id: Uuid) -> Result<u32> {
    let route = transfer(TransferDirection::Download, username, id).then("position");
    json(self.get(route)).await
  }

  /// `POST /transfers/downloads/{username}`: enqueues files from a user.
  pub async fn enqueue(
    &self,
    username: &str,
    files: &[QueueDownloadRequest],
  ) -> Result<QueueDownloadResponse> {
    let route = transfers(TransferDirection::Download).segment(username);
    json(self.post(route).json(files)).await
  }

  /// `POST /transfers/downloads/batches`: enqueues files from a user as a batch.
  pub async fn enqueue_batch(
    &self,
    request: &EnqueueDownloadBatchRequest,
  ) -> Result<EnqueueDownloadBatchResponse> {
    let route = transfers(TransferDirection::Download).then("batches");
    json(self.post(route).json(request)).await
  }

  /// `GET /transfers/downloads/batches/{id}`: one batch with its downloads.
  pub async fn batch(&self, id: Uuid, include_removed: bool) -> Result<Batch> {
    let route = transfers(TransferDirection::Download)
      .then("batches")
      .segment(&id.to_string())
      .query(&IncludeRemovedQuery { include_removed })?;
    json(self.get(route)).await
  }

  /// `GET /transfers/uploads/{username}/{id}/why`: why a queued upload waits.
  pub async fn upload_queue_status(&self, username: &str, id: Uuid) -> Result<UploadQueueStatus> {
    let route = transfer(TransferDirection::Upload, username, id).then("why");
    json(self.get(route)).await
  }

  /// `POST /transfers/uploads/{username}/{id}/bump`: moves a queued upload to the front.
  pub async fn bump_upload(&self, username: &str, id: Uuid) -> Result<()> {
    let route = transfer(TransferDirection::Upload, username, id).then("bump");
    done(self.post(route)).await
  }
}
