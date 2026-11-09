//! Why a queued upload waits: its place in the upload queue, the slots in use and the user's
//! limits.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Where a queued upload stands in the upload queue and what it waits for, worked out without
/// changing the queue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadQueueStatus {
  /// the upload's 0-based place among its group's uploads, uploads in progress counted; null
  /// when the queue does not hold the upload
  pub position: Option<u32>,
  /// the group whose queue the user waits in
  #[schema(example = "default")]
  pub group: String,
  /// uploads in progress in that queue
  pub group_slots_used: u32,
  /// how many uploads that queue may run at once
  pub group_slots: u32,
  /// uploads in progress over every group
  pub global_slots_used: u32,
  /// how many uploads may run at once over every group
  pub global_slots: u32,
  /// uploads to this user in progress
  pub user_active: u32,
  /// the user's figures against the group's limits
  pub limits: UploadLimits,
}

/// A user's upload figures against the limits of their group, per window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadLimits {
  /// uploads queued or in progress
  pub queued: UploadLimitWindow,
  /// uploads started in the trailing 24 hours
  pub daily: UploadLimitWindow,
  /// uploads started in the trailing 7 days
  pub weekly: UploadLimitWindow,
}

/// The figures of one limit window. Succeeded uploads count toward `files` and `bytes` of the
/// daily and weekly windows, failed ones toward `failures`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadLimitWindow {
  /// files
  pub files: UploadLimitUsage,
  /// bytes, the limit converted from megabytes of 1 000 000 bytes
  pub bytes: UploadLimitUsage,
  /// failed uploads, null for the queued window, which has no failure limit
  pub failures: Option<UploadLimitUsage>,
}

/// How much of one limit is used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadLimitUsage {
  /// how much the user has used
  pub used: u64,
  /// the limit, null when there is none
  pub limit: Option<u64>,
}

/// Files of ours to send to a user who did not ask for them, as `POST /transfers/uploads/{u}`
/// takes them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SendFilesRequest {
  /// the files, by the names other users see them under
  #[schema(example = json!(["@@abcde\\Music\\Artist\\Album\\01 - Track.flac"]))]
  pub files: Vec<String>,
}

/// What sending files to a user did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SendFilesResponse {
  /// the files queued to be offered to the user
  pub queued: Vec<String>,
  /// the files not queued, and why
  pub failed: Vec<super::EnqueueFailure>,
}
