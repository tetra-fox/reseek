//! The transfers calls of the API: the download calls go to the download engine, the upload
//! calls to the upload engine.

use std::sync::Arc;

use api_types::Page;
use api_types::transfers::{
  BulkRequest, BulkResult, DebugTransfers, EnqueueDownloadJobRequest, EnqueueDownloadJobResponse,
  Transfer, TransferListQuery, UploadQueueStatus, UploadsQuery, UserResponse,
};
use async_trait::async_trait;
use uuid::Uuid;

use super::downloads::Downloads;
use super::uploads::Uploads;
use crate::api::error::ApiError;
use crate::services::TransferService;

/// Both engines, behind the transfers calls.
pub struct Transfers {
  /// the download engine
  pub downloads: Arc<Downloads>,
  /// the upload engine
  pub uploads: Arc<Uploads>,
}

#[async_trait]
impl TransferService for Transfers {
  async fn downloads(&self, include_removed: bool) -> Result<Vec<UserResponse>, ApiError> {
    self.downloads.downloads(include_removed).await
  }

  async fn download_list(&self, query: TransferListQuery) -> Result<Page<Transfer>, ApiError> {
    self.downloads.download_list(query).await
  }

  async fn user_downloads(&self, username: &str) -> Result<UserResponse, ApiError> {
    self.downloads.user_downloads(username).await
  }

  async fn download(
    &self,
    username: &str,
    id: Uuid,
    include_history: bool,
  ) -> Result<Transfer, ApiError> {
    TransferService::download(&*self.downloads, username, id, include_history).await
  }

  async fn place_in_queue(&self, username: &str, id: Uuid) -> Result<u32, ApiError> {
    TransferService::place_in_queue(&*self.downloads, username, id).await
  }

  async fn enqueue_job(
    &self,
    request: EnqueueDownloadJobRequest,
  ) -> Result<EnqueueDownloadJobResponse, ApiError> {
    self.downloads.enqueue_job(request).await
  }

  async fn cancel_download(&self, username: &str, id: Uuid, remove: bool) -> Result<(), ApiError> {
    self.downloads.cancel_download(username, id, remove).await
  }

  async fn remove_completed_downloads(&self) -> Result<(), ApiError> {
    self.downloads.remove_completed_downloads().await
  }

  async fn bulk_downloads(&self, request: BulkRequest) -> Result<BulkResult, ApiError> {
    self.downloads.bulk_downloads(request).await
  }

  async fn uploads(&self, query: UploadsQuery) -> Result<Vec<UserResponse>, ApiError> {
    self.uploads.uploads(query).await
  }

  async fn upload_list(&self, query: TransferListQuery) -> Result<Page<Transfer>, ApiError> {
    self.uploads.upload_list(query).await
  }

  async fn user_uploads(&self, username: &str) -> Result<UserResponse, ApiError> {
    self.uploads.user_uploads(username).await
  }

  async fn upload(
    &self,
    username: &str,
    id: Uuid,
    include_history: bool,
  ) -> Result<Transfer, ApiError> {
    self.uploads.upload(username, id, include_history).await
  }

  async fn cancel_upload(&self, username: &str, id: Uuid, remove: bool) -> Result<(), ApiError> {
    self.uploads.cancel_upload(username, id, remove).await
  }

  async fn remove_completed_uploads(&self) -> Result<(), ApiError> {
    self.uploads.remove_completed_uploads().await
  }

  async fn bulk_uploads(&self, request: BulkRequest) -> Result<BulkResult, ApiError> {
    self.uploads.bulk_uploads(request).await
  }

  async fn upload_queue_status(
    &self,
    username: &str,
    id: Uuid,
  ) -> Result<UploadQueueStatus, ApiError> {
    self.uploads.upload_queue_status(username, id).await
  }

  async fn bump(&self, username: &str, id: Uuid) -> Result<(), ApiError> {
    self.uploads.bump(username, id).await
  }

  async fn client_transfers(&self) -> Result<DebugTransfers, ApiError> {
    self.downloads.client_transfers().await
  }
}
