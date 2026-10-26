//! Uploads, downloads and the user groups uploads are queued by.
//!
//! A slot count or speed limit that is not set has no limit. Speed limits are in KiB/s.

use std::collections::BTreeMap;
use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::{DestinationExists, QueueStrategy, RequestStyle, RetryPartial};

/// Uploads, downloads and the user groups uploads are queued by.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Transfers {
  /// Uploads to other users.
  pub upload: Upload,
  /// Downloads from other users.
  pub download: Download,
  /// The groups that decide each user's upload queue, slots and limits.
  pub groups: Groups,
  /// What the transfers page does on its own.
  pub ui: TransfersUi,
}

/// Uploads to other users.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Upload {
  /// How many uploads run at the same time over every group. A lower number lets the uploads
  /// that run finish and starts no new one until fewer run.
  #[cascade(env = "RESEEK_UPLOAD_SLOTS", cli = "--upload-slots")]
  pub slots: u32,
  /// The total upload speed in KiB/s; no limit when not set.
  #[cascade(env = "RESEEK_UPLOAD_SPEED_LIMIT", cli = "--upload-speed-limit")]
  pub speed_limit: Option<u32>,
  /// How much each user may queue and fetch, unless their group says otherwise.
  pub limits: Limits,
}

impl Default for Upload {
  fn default() -> Self {
    Self {
      slots: 10,
      speed_limit: None,
      limits: Limits::default(),
    }
  }
}

/// How much one user may queue and fetch, over three windows. A limit that is not set does not
/// apply.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Limits {
  /// What may be queued at once.
  pub queued: Limit,
  /// What may be fetched in a day.
  pub daily: Limit,
  /// What may be fetched in a week.
  pub weekly: Limit,
}

/// A limit on files, megabytes and failed transfers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Limit {
  /// The most files.
  pub files: Option<u32>,
  /// The most megabytes.
  pub megabytes: Option<u32>,
  /// The most failed transfers.
  pub failures: Option<u32>,
}

/// Downloads from other users.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Download {
  /// How many downloads run at the same time; no limit when not set. A lower number lets the
  /// downloads that run finish and starts no new one until fewer run.
  #[cascade(env = "RESEEK_DOWNLOAD_SLOTS", cli = "--download-slots")]
  pub slots: Option<u32>,
  /// The total download speed in KiB/s; no limit when not set.
  #[cascade(env = "RESEEK_DOWNLOAD_SPEED_LIMIT", cli = "--download-speed-limit")]
  pub speed_limit: Option<u32>,
  /// How a file is asked for: `queue`, the queue request every current client sends, or
  /// `transfer_request`, the older transfer request the peer answers.
  pub request_style: RequestStyle,
  /// Retrying failed downloads.
  pub retry: Retry,
  /// Where finished downloads go.
  pub destination: Destination,
}

/// Retrying failed downloads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Retry {
  /// How many times a failed download is tried again.
  pub attempts: u32,
  /// How long to wait before the first retry; each later wait doubles, at least a second.
  #[serde(with = "humantime_serde")]
  pub delay: Duration,
  /// The longest wait between retries, at least 30 seconds.
  #[serde(with = "humantime_serde")]
  pub max_delay: Duration,
  /// What a retry does with the part already downloaded: `resume` or `overwrite`.
  pub partial: RetryPartial,
}

impl Default for Retry {
  fn default() -> Self {
    Self {
      attempts: 3,
      delay: Duration::from_secs(5),
      max_delay: Duration::from_secs(60),
      partial: RetryPartial::default(),
    }
  }
}

/// Where finished downloads go.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Destination {
  /// The relative directory under the downloads directory a file is moved to, with
  /// `${SOURCE_USERNAME}`, `${SOURCE_PATH}`, `${SOURCE_DIRECTORY}`, `${BATCH_ID}`,
  /// `${BATCH_EXTERNAL_ID}`, `${SEARCH_ID}` and `${SEARCH_TEXT}` filled in.
  pub subdirectory: Option<String>,
  /// What happens when the file already exists: `rename` or `overwrite`.
  pub exists: DestinationExists,
  /// The permissions of downloaded files and the directories made for them.
  pub permissions: Permissions,
}

impl Default for Destination {
  fn default() -> Self {
    Self {
      subdirectory: Some("${SOURCE_DIRECTORY}".to_string()),
      exists: DestinationExists::default(),
      permissions: Permissions::default(),
    }
  }
}

/// The permissions of downloaded files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Permissions {
  /// The mode in chmod's octal form, such as `644`; the system's default when not set. No
  /// effect on Windows.
  pub mode: Option<String>,
}

/// The groups that decide each user's upload queue, slots and limits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Groups {
  /// Users in no other group.
  pub default: Group,
  /// Users who share too little.
  pub leechers: Leechers,
  /// Users who are never served.
  pub blacklisted: Blacklisted,
  /// Groups of the user's own, by name.
  #[cascade(user_keyed)]
  pub user_defined: BTreeMap<String, UserDefinedGroup>,
}

/// A built-in group's uploads.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Group {
  /// How the group's uploads are queued and limited.
  pub upload: GroupUpload,
}

/// How a group's uploads are queued and limited.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct GroupUpload {
  /// The group's priority; a lower number is served first.
  pub priority: u32,
  /// The order the group's queue is served in: `roundrobin` or `firstinfirstout`.
  pub strategy: QueueStrategy,
  /// How many of the group's uploads run at once; no limit beyond the global one when not
  /// set.
  pub slots: Option<u32>,
  /// The group's total upload speed in KiB/s; no limit when not set.
  pub speed_limit: Option<u32>,
  /// The group's per-user limits, replacing the global ones.
  pub limits: Limits,
}

impl Default for GroupUpload {
  fn default() -> Self {
    Self {
      priority: 1,
      strategy: QueueStrategy::default(),
      slots: None,
      speed_limit: None,
      limits: Limits::default(),
    }
  }
}

/// Users who share too little.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Leechers {
  /// How the group's uploads are queued and limited.
  pub upload: GroupUpload,
  /// What a user must share not to be in the group.
  pub thresholds: LeecherThresholds,
}

/// What a user must share not to count as a leecher.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct LeecherThresholds {
  /// The fewest shared files.
  pub files: u32,
  /// The fewest shared directories.
  pub directories: u32,
}

impl Default for LeecherThresholds {
  fn default() -> Self {
    Self {
      files: 1,
      directories: 1,
    }
  }
}

/// Users who are never served.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Blacklisted {
  /// Usernames.
  pub members: Vec<String>,
  /// Regular expressions matched against usernames.
  pub patterns: Vec<String>,
  /// Networks whose addresses are refused.
  pub cidrs: Vec<String>,
}

/// A group of the user's own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct UserDefinedGroup {
  /// How the group's uploads are queued and limited.
  pub upload: GroupUpload,
  /// The usernames in the group.
  pub members: Vec<String>,
}

/// What the transfers page does on its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct TransfersUi {
  /// How often the page asks for the queue place of visible remotely queued downloads, at
  /// least 30 seconds.
  #[cascade(env = "RESEEK_TRANSFERS_UI_QUEUE_REFRESH")]
  #[serde(with = "humantime_serde")]
  pub queue_refresh: Duration,
}

impl Default for TransfersUi {
  fn default() -> Self {
    Self {
      queue_refresh: Duration::from_secs(120),
    }
  }
}
