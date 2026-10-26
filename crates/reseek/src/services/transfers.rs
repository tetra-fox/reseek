//! Downloads and uploads.

use api_types::Page;
use api_types::transfers::{
  Batch, BulkRequest, BulkResult, DebugTransfers, EnqueueDownloadBatchRequest,
  EnqueueDownloadBatchResponse, QueueDownloadRequest, QueueDownloadResponse, Transfer,
  TransferListQuery, UploadQueueStatus, UploadsQuery, UserResponse,
};
use async_trait::async_trait;
use uuid::Uuid;

use super::not_yet;
use crate::api::error::ApiError;

const DOMAIN: &str = "transfers";

/// Listing, enqueueing, cancelling and removing transfers. Every call that takes a username and
/// an id finds the transfer by both, and answers 404 when no transfer of that direction has
/// them.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait TransferService: Send + Sync {
  /// Every download, grouped by user and remote directory.
  async fn downloads(&self, include_removed: bool) -> Result<Vec<UserResponse>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A page of downloads, flat.
  async fn download_list(&self, query: TransferListQuery) -> Result<Page<Transfer>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The downloads from one user not removed, grouped by remote directory; 404 when there are
  /// none.
  async fn user_downloads(&self, username: &str) -> Result<UserResponse, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// One download, with the states it went through when asked for.
  async fn download(
    &self,
    username: &str,
    id: Uuid,
    include_history: bool,
  ) -> Result<Transfer, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Asks the peer where a queued download is in its queue, records the answer and returns it.
  /// A download its peer has not queued is a conflict, and a peer that does not answer in the
  /// connection's inactivity timeout a 504.
  async fn place_in_queue(&self, username: &str, id: Uuid) -> Result<u32, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Enqueues `files` from `username`, each checked by the API. The user being offline or
  /// blocked by the configuration is a 404 with the same message for both, a user blocked
  /// from downloads through the API a conflict; two enqueues already running is a 429.
  async fn enqueue(
    &self,
    username: &str,
    files: Vec<QueueDownloadRequest>,
  ) -> Result<QueueDownloadResponse, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Makes a batch and enqueues its files, as one change: a failure leaves no batch behind.
  /// `request` has passed the API's checks; a batch with its id already existing is a conflict.
  async fn enqueue_batch(
    &self,
    request: EnqueueDownloadBatchRequest,
  ) -> Result<EnqueueDownloadBatchResponse, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// One batch with its downloads.
  async fn batch(&self, id: Uuid, include_removed: bool) -> Result<Batch, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Cancels a download and, with `remove`, removes it from the listings once it has
  /// completed.
  async fn cancel_download(&self, username: &str, id: Uuid, remove: bool) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Removes every completed download from the listings.
  async fn remove_completed_downloads(&self) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Applies one action to several downloads.
  async fn bulk_downloads(&self, request: BulkRequest) -> Result<BulkResult, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Every upload in `query`'s range, grouped by user and remote directory.
  async fn uploads(&self, query: UploadsQuery) -> Result<Vec<UserResponse>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A page of uploads, flat.
  async fn upload_list(&self, query: TransferListQuery) -> Result<Page<Transfer>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The uploads to one user not removed, grouped by remote directory, with no directories
  /// when there are none.
  async fn user_uploads(&self, username: &str) -> Result<UserResponse, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// One upload, with the states it went through when asked for.
  async fn upload(
    &self,
    username: &str,
    id: Uuid,
    include_history: bool,
  ) -> Result<Transfer, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Cancels an upload and, with `remove`, removes it from the listings once it has completed.
  async fn cancel_upload(&self, username: &str, id: Uuid, remove: bool) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Removes every completed upload from the listings.
  async fn remove_completed_uploads(&self) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Applies one action to several uploads.
  async fn bulk_uploads(&self, request: BulkRequest) -> Result<BulkResult, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Why a queued upload waits, worked out without changing the queue.
  async fn upload_queue_status(
    &self,
    username: &str,
    id: Uuid,
  ) -> Result<UploadQueueStatus, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Moves a queued upload to the front of its group's queue.
  async fn bump(&self, username: &str, id: Uuid) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The Soulseek client's own records of the transfers it runs, for diagnosis.
  async fn client_transfers(&self) -> Result<DebugTransfers, ApiError> {
    Err(not_yet(DOMAIN))
  }
}
