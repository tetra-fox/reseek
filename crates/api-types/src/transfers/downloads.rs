//! Enqueueing downloads, one by one, as a batch or as a job of several batches, the batch
//! record, and the end of a job.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{Transfer, TransferDirection};
use crate::search::FileAttribute;

/// One file to download from the user named in the path. The body of the call is a list of
/// these.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueDownloadRequest {
  /// the remote filename exactly as a search or browse response gave it
  #[schema(example = "@@abcde\\Music\\Artist\\Album\\01 - Track.flac")]
  pub filename: String,
  /// the file's size in bytes as the response gave it, 0 when unknown
  #[serde(default)]
  pub size: u64,
}

/// What enqueueing a list of files did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueDownloadResponse {
  /// the downloads that were created
  pub enqueued: Vec<Transfer>,
  /// the files that were not enqueued, and why
  pub failed: Vec<EnqueueFailure>,
}

/// A file that was not enqueued.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueFailure {
  /// the remote filename as it was asked for
  pub filename: String,
  /// why it was not enqueued, such as `Skipped: Already in progress`
  pub message: String,
}

/// Downloads several files from one user as one batch, which can give them a common
/// destination.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueDownloadBatchRequest {
  /// the batch's id, not the nil id; a new one is made when null
  pub id: Option<Uuid>,
  /// the search the files were found by, null when there is none
  pub search_id: Option<Uuid>,
  /// the user to download from
  #[schema(example = "alice")]
  pub username: String,
  /// the files, at least one, no filename twice
  pub files: Vec<EnqueueDownloadBatchItem>,
  /// where the files go and how the caller tags the batch
  #[serde(default)]
  pub options: BatchOptions,
}

/// Downloads several batches as one job: everything picked in one go, such as an album whose
/// discs go to folders of their own, or files from several users picked from one search. The
/// job is told of once, by a `transfers.job_finished` frame, when every one of its downloads
/// has ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueDownloadJobRequest {
  /// the batches, at least one, enqueued in this order
  pub batches: Vec<EnqueueDownloadBatchRequest>,
}

/// What enqueueing a job did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueDownloadJobResponse {
  /// the job's id, which its `transfers.job_finished` frame carries
  pub id: Uuid,
  /// what came of each batch, in the order of the request
  pub batches: Vec<EnqueuedJobBatch>,
}

/// What came of one batch of a job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueuedJobBatch {
  /// the user the batch's files come from
  pub username: String,
  /// the batch made, null when it was not
  pub batch_id: Option<Uuid>,
  /// the files of the batch made that were not enqueued, and why
  pub failures: Vec<EnqueueFailure>,
  /// why the batch was not made, such as its user being offline; null when it was
  pub error: Option<crate::ErrorBody>,
}

/// A download job ended: every download queued in one go has succeeded, failed or been
/// canceled. Sent once per job, as the `transfers.job_finished` frame; a job left with no
/// download, every one removed, is not told of.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJobFinished {
  /// the job's id
  pub id: Uuid,
  /// the users the files came from, in the order they were asked of
  pub usernames: Vec<String>,
  /// the first file asked for, as the peer shares it
  #[schema(example = "@@abcde\\Music\\Artist\\Album\\01 - Track.flac")]
  pub filename: String,
  /// the deepest remote folder holding every file, when they all come from one user; empty
  /// when they do not, or share no folder
  #[schema(example = "@@abcde\\Music\\Artist\\Album")]
  pub folder: String,
  /// how many downloads the job has, removed ones left out
  pub file_count: u32,
  /// how many of them succeeded
  pub succeeded_count: u32,
  /// how many failed, canceled ones left out
  pub failed_count: u32,
  /// how many were canceled
  pub canceled_count: u32,
}

/// One file of a batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueDownloadBatchItem {
  /// the remote filename exactly as a search or browse response gave it
  #[schema(example = "@@abcde\\Music\\Artist\\Album\\01 - Track.flac")]
  pub filename: String,
  /// the file's size in bytes as the response gave it
  pub size: u64,
  /// the file's attributes as the response gave them, kept to be shown with the download
  #[serde(default)]
  pub attributes: Vec<FileAttribute>,
}

/// Where a batch's files go and how the caller tags it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchOptions {
  /// a relative path under the downloads directory that every file of the batch goes to,
  /// in place of the directory the download pattern gives; null for the pattern
  #[schema(example = "Artist/Album")]
  pub destination: Option<String>,
  /// a tag of the caller's choosing, given to the download pattern; null for none
  pub external_id: Option<String>,
}

/// What enqueueing a batch did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueDownloadBatchResponse {
  /// the batch with the downloads that were created
  pub batch: Batch,
  /// the files that were not enqueued, and why
  pub failures: Vec<EnqueueFailure>,
}

/// Downloads from one user enqueued together. A batch is never changed after it is made; its
/// counts and state are worked out from its transfers when it is read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
  /// the batch's id
  pub id: Uuid,
  /// the search the files were found by, null when there is none
  pub search_id: Option<Uuid>,
  /// the user the files come from
  pub username: String,
  /// always `Download`
  pub direction: TransferDirection,
  /// when the batch was made
  pub created_at: DateTime<Utc>,
  /// the batch's transfers, without removed ones unless they were asked for
  pub transfers: Vec<Transfer>,
  /// where the files go and how the caller tagged the batch
  pub options: BatchOptions,
  /// how many transfers the batch has
  pub file_count: u32,
  /// how many of them completed, whatever the outcome
  pub completed_count: u32,
  /// how many of them completed other than successfully
  pub failed_count: u32,
  /// the sizes of the transfers added up, in bytes
  pub bytes_total: u64,
  /// the bytes moved by the transfers added up
  pub bytes_transferred: u64,
  /// where the batch is as a whole
  pub state: BatchState,
}

/// Where a batch is as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum BatchState {
  /// no transfer has started
  Pending,
  /// some transfers have started and not every one has completed
  InProgress,
  /// every transfer completed successfully
  Completed,
  /// every transfer completed and at least one did not succeed
  Failed,
}
