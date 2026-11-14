//! Enqueueing downloads as a download job: everything picked in one go, in groups of files from
//! one user each, and the end of a job.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::Transfer;

/// One file to download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueDownloadFile {
  /// the remote filename exactly as a search or browse response gave it
  #[schema(example = "@@abcde\\Music\\Artist\\Album\\01 - Track.flac")]
  pub filename: String,
  /// the file's size in bytes as the response gave it, 0 when unknown
  #[serde(default)]
  pub size: u64,
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

/// Files from one user, one group of a download job. Each of its downloads keeps the group's
/// destination, folder and search.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueDownloadGroup {
  /// the user to download from
  #[schema(example = "alice")]
  pub username: String,
  /// the files, at least one, no filename twice
  pub files: Vec<EnqueueDownloadFile>,
  /// a relative path under the downloads directory that every file of the group goes to as
  /// written, in place of the directory the download pattern gives; null for the pattern. Not
  /// given with `folder`
  #[serde(default)]
  #[schema(example = "Artist/Album")]
  pub destination: Option<String>,
  /// the remote folder the files were picked in, such as a folder downloaded whole, which every
  /// file is under: each file's folders from this one down, `Album/CD1` for
  /// `@@abcde\Music\Album\CD1\01.flac` in `@@abcde\Music\Album`, take the place of the download
  /// pattern's `${SOURCE_DIRECTORY}`, or go after the pattern's directory when it names neither
  /// `${SOURCE_DIRECTORY}` nor `${SOURCE_PATH}`, so the folder keeps its tree wherever the pattern
  /// puts it. A root such as `/` names its files from their first folder down. Null for each
  /// file's own folder name
  #[serde(default)]
  #[schema(example = "@@abcde\\Music\\Artist\\Album")]
  pub folder: Option<String>,
  /// the search the files were found by, null when there is none
  #[serde(default)]
  pub search_id: Option<Uuid>,
}

/// Downloads everything picked in one go as one download job, such as a file, an album whose
/// discs go to folders of their own, or files from several users picked from one search. The
/// job is told of once, by a `transfers.job_finished` frame, when every one of its downloads
/// has ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueDownloadJobRequest {
  /// the groups, at least one, enqueued in this order
  pub groups: Vec<EnqueueDownloadGroup>,
}

/// What enqueueing a download job did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueDownloadJobResponse {
  /// the job's id, which its downloads and its `transfers.job_finished` frame carry
  pub id: Uuid,
  /// what came of each group, in the order of the request
  pub groups: Vec<EnqueuedDownloadGroup>,
}

/// What came of one group of a download job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnqueuedDownloadGroup {
  /// the user the group's files come from
  pub username: String,
  /// the downloads made
  pub enqueued: Vec<Transfer>,
  /// the files of the group that were not enqueued, and why
  pub failures: Vec<EnqueueFailure>,
  /// why none of the group's files was enqueued, such as its user being offline; null when
  /// the group was tried
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
