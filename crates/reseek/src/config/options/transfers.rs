//! Uploads, downloads and the user groups uploads are queued by.
//!
//! A slot count or speed limit that is not set has no limit. Speed limits are in KiB/s.

use std::collections::BTreeMap;
use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::{DestinationExists, QueueStrategy, RequestStyle, RetryPartial};

/// Uploads, downloads and upload groups.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Transfers {
  /// Uploads to other users.
  pub upload: Upload,
  /// Downloads from other users.
  pub download: Download,
  /// User groups that determine the upload queue, slots and limits of each user.
  pub groups: Groups,
  /// Automatic behavior of the transfers page.
  pub ui: TransfersUi,
}

impl Transfers {
  /// The total upload speed limit in KiB/s, the alternative one when `alternative`. No limit
  /// when `None`.
  pub fn upload_speed_limit(&self, alternative: bool) -> Option<u32> {
    if alternative {
      self.upload.alt_speed_limit
    } else {
      self.upload.speed_limit
    }
  }

  /// The total download speed limit in KiB/s, the alternative one when `alternative`. No limit
  /// when `None`.
  pub fn download_speed_limit(&self, alternative: bool) -> Option<u32> {
    if alternative {
      self.download.alt_speed_limit
    } else {
      self.download.speed_limit
    }
  }
}

/// Uploads to other users.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Upload {
  /// Maximum number of uploads running at once, across all groups. Lowering it lets running uploads
  /// finish and starts no new one until fewer run.
  #[cascade(env = "RESEEK_UPLOAD_SLOTS", cli = "--upload-slots")]
  pub slots: u32,
  /// Total upload speed limit in KiB/s. No limit when not set.
  #[cascade(env = "RESEEK_UPLOAD_SPEED_LIMIT", cli = "--upload-speed-limit")]
  pub speed_limit: Option<u32>,
  /// Total upload speed limit in KiB/s while the alternative limits are on. No limit when not
  /// set.
  #[cascade(env = "RESEEK_UPLOAD_ALT_SPEED_LIMIT")]
  pub alt_speed_limit: Option<u32>,
  /// Per-user queue and download limits, unless the group of the user sets its own. Only the
  /// number of queued files is limited by default, so one user cannot queue every shared file.
  pub limits: Limits,
}

impl Default for Upload {
  fn default() -> Self {
    Self {
      slots: 10,
      speed_limit: None,
      alt_speed_limit: None,
      limits: Limits {
        queued: Limit {
          files: Some(1000),
          ..Limit::default()
        },
        ..Limits::default()
      },
    }
  }
}

/// Limits on what one user may queue and download, over three windows. A limit that is not set
/// does not apply.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Limits {
  /// Limits on what may be queued at once.
  pub queued: Limit,
  /// Limits on what may be downloaded in a day.
  pub daily: Limit,
  /// Limits on what may be downloaded in a week.
  pub weekly: Limit,
}

/// A limit on files, megabytes and failed transfers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Limit {
  /// Maximum number of files.
  pub files: Option<u32>,
  /// Maximum number of megabytes.
  pub megabytes: Option<u32>,
  /// Maximum number of failed transfers.
  pub failures: Option<u32>,
}

/// Downloads from other users.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Download {
  /// Maximum number of downloads running at once. No limit when not set. Lowering it lets running
  /// downloads finish and starts no new one until fewer run.
  #[cascade(env = "RESEEK_DOWNLOAD_SLOTS", cli = "--download-slots")]
  pub slots: Option<u32>,
  /// Total download speed limit in KiB/s. No limit when not set.
  #[cascade(env = "RESEEK_DOWNLOAD_SPEED_LIMIT", cli = "--download-speed-limit")]
  pub speed_limit: Option<u32>,
  /// Total download speed limit in KiB/s while the alternative limits are on. No limit when
  /// not set.
  #[cascade(env = "RESEEK_DOWNLOAD_ALT_SPEED_LIMIT")]
  pub alt_speed_limit: Option<u32>,
  /// How a file is requested: `queue`, as every current client does, or `transfer_request`, the
  /// older style.
  pub request_style: RequestStyle,
  /// Retries of failed downloads.
  pub retry: Retry,
  /// Destination of finished downloads.
  pub destination: Destination,
  /// Regular expressions matched against each file's remote path, ignoring case. Matching files are
  /// skipped in folder and selection downloads, never when downloaded on their own.
  #[cascade(env = "RESEEK_DOWNLOAD_FILTERS")]
  pub filters: Vec<String>,
}

impl Default for Download {
  fn default() -> Self {
    Self {
      slots: None,
      speed_limit: None,
      alt_speed_limit: None,
      request_style: RequestStyle::default(),
      retry: Retry::default(),
      destination: Destination::default(),
      filters: vec![
        r"\.(exe|msi|scr|com|bat|lnk)$".to_string(),
        r"(^|[\\/])(thumbs\.db|desktop\.ini|\.ds_store)$".to_string(),
      ],
    }
  }
}

/// Retries of failed downloads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Retry {
  /// Number of attempts per download, including the first. `1` disables retries.
  pub attempts: u32,
  /// Wait before the first retry. Each later wait doubles. Minimum: 1 second.
  #[serde(with = "crate::config::duration")]
  pub delay: Duration,
  /// Longest wait between retries. Minimum: 30 seconds.
  #[serde(with = "crate::config::duration")]
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

/// Destination of finished downloads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Destination {
  /// Directory inside the downloads directory that finished files go to. `${SOURCE_USERNAME}`,
  /// `${SOURCE_PATH}`, `${SOURCE_DIRECTORY}`, `${JOB_ID}` (everything queued together),
  /// `${SEARCH_ID}` and `${SEARCH_TEXT}` are replaced with their values.
  pub subdirectory: Option<String>,
  /// What happens when the file already exists: `rename` or `overwrite`.
  pub exists: DestinationExists,
  /// Permissions of downloaded files and of the directories created for them.
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

/// Permissions of downloaded files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Permissions {
  /// File mode in octal chmod form, such as `644`. The system default applies when not set.
  /// Has no effect on Windows.
  pub mode: Option<String>,
}

/// User groups that determine the upload queue, slots and limits of each user.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Groups {
  /// Users in no other group.
  pub default: Group,
  /// Users who share too little.
  pub leechers: Leechers,
  /// Users who are never served.
  pub blacklisted: Blacklisted,
  /// User-defined groups by name.
  #[cascade(user_keyed)]
  pub user_defined: BTreeMap<String, UserDefinedGroup>,
}

/// Uploads of a built-in group.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Group {
  /// Upload queue and limits of the group.
  pub upload: GroupUpload,
}

/// Upload queue and limits of a group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct GroupUpload {
  /// Priority of the group. A lower number is served first.
  pub priority: u32,
  /// How the group's queue is served: `roundrobin`, one file per user in turn, or
  /// `firstinfirstout`, in the order queued.
  pub strategy: QueueStrategy,
  /// Maximum number of uploads of the group running at once. Only the global limit applies
  /// when not set.
  pub slots: Option<u32>,
  /// Total upload speed limit of the group in KiB/s. No limit when not set.
  pub speed_limit: Option<u32>,
  /// Per-user limits of the group, replacing the global ones.
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
  /// Upload queue and limits of the group.
  pub upload: GroupUpload,
  /// Minimum a user must share to stay out of the group.
  pub thresholds: LeecherThresholds,
}

/// Minimum a user must share to not count as a leecher.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct LeecherThresholds {
  /// Minimum number of shared files.
  pub files: u32,
  /// Minimum number of shared directories.
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
  /// Networks whose users are refused. Their private messages are also dropped and their room
  /// messages hidden, once their address is known.
  pub cidrs: Vec<String>,
}

/// A user-defined group.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct UserDefinedGroup {
  /// Upload queue and limits of the group.
  pub upload: GroupUpload,
  /// Usernames in the group.
  pub members: Vec<String>,
}

/// Automatic behavior of the transfers page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct TransfersUi {
  /// How often the downloads page updates the queue position of the downloads on screen. Minimum:
  /// 30 seconds.
  #[cascade(env = "RESEEK_TRANSFERS_UI_QUEUE_REFRESH")]
  #[serde(with = "crate::config::duration")]
  pub queue_refresh: Duration,
}

impl Default for TransfersUi {
  fn default() -> Self {
    Self {
      queue_refresh: Duration::from_secs(120),
    }
  }
}
