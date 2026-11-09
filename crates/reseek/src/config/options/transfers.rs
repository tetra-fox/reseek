//! Uploads, downloads and the user groups uploads are queued by.
//!
//! A slot count or speed limit that is not set has no limit. Speed limits are in KiB/s.

use std::collections::BTreeMap;
use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::{
  DestinationExists, GroupColor, QueueStrategy, RequestStyle, RetryPartial,
};

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

/// Limits on what one user may queue and download, over three windows.
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
  /// Maximum number of files. `0` for no limit.
  pub files: Option<u32>,
  /// Maximum number of megabytes. `0` for no limit.
  pub megabytes: Option<u32>,
  /// Maximum number of failed transfers. `0` for no limit.
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
  /// `${SEARCH_ID}` and `${SEARCH_TEXT}` are replaced with their values. `{}` puts files
  /// straight into the downloads directory. A download queued with a directory of its own goes
  /// there instead.
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

/// The group of users the server calls privileged.
pub const PRIVILEGED_GROUP: &str = "privileged";
/// The group of users who share too little.
pub const LEECHERS_GROUP: &str = "leechers";
/// The group of our buddies.
pub const BUDDIES_GROUP: &str = "buddies";
/// The group of everyone in no group with upload settings of its own.
pub const DEFAULT_GROUP: &str = "default";
/// The names a user-defined group cannot have.
pub const BUILTIN_GROUPS: [&str; 5] = [
  PRIVILEGED_GROUP,
  LEECHERS_GROUP,
  BUDDIES_GROUP,
  DEFAULT_GROUP,
  "blacklisted",
];

/// Whether `name` can name a group: 1 to 32 of `a-z`, `0-9`, `_` and `-`.
pub fn is_group_name(name: &str) -> bool {
  let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-';
  !name.is_empty() && name.chars().count() <= 32 && name.chars().all(allowed)
}

/// User groups that determine the upload queue, slots and limits of each user. Each setting
/// comes from the highest group in the order the user is in that sets it, else from `default`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Groups {
  /// Order of the groups, highest first. Privileged users always go first and `default`, which
  /// is everyone, always last. Must list `buddies` and `leechers`; a user-defined group not
  /// listed goes just above `default`, and a name no group has is passed over.
  pub order: Vec<String>,
  /// Everyone: the settings no higher group sets.
  pub default: Group,
  /// Users who share too little.
  pub leechers: Leechers,
  /// Our buddies.
  pub buddies: Buddies,
  /// Users who are never served.
  pub blacklisted: Blacklisted,
  /// User-defined groups by name. A name is 1 to 32 characters from `a-z`, `0-9`, `_` and `-`.
  #[cascade(user_keyed)]
  pub user_defined: BTreeMap<String, UserDefinedGroup>,
}

impl Default for Groups {
  fn default() -> Self {
    Self {
      order: [BUDDIES_GROUP, LEECHERS_GROUP].map(str::to_string).into(),
      default: Group::default(),
      leechers: Leechers::default(),
      buddies: Buddies::default(),
      blacklisted: Blacklisted::default(),
      user_defined: BTreeMap::new(),
    }
  }
}

impl Groups {
  /// Every group but privileged and `default`, highest first: those of `order` that exist, then
  /// each user-defined group it leaves out, by name.
  pub fn ordered(&self) -> Vec<&str> {
    let mut ordered: Vec<&str> = self
      .order
      .iter()
      .map(String::as_str)
      .filter(|name| {
        [BUDDIES_GROUP, LEECHERS_GROUP].contains(name) || self.user_defined.contains_key(*name)
      })
      .collect();
    let left_out: Vec<&str> = self
      .user_defined
      .keys()
      .map(String::as_str)
      .filter(|name| !ordered.contains(name))
      .collect();
    ordered.extend(left_out);
    ordered
  }

  /// The upload settings of the group `name`; `None` for privileged and a name no group has.
  pub fn upload(&self, name: &str) -> Option<&GroupUpload> {
    match name {
      DEFAULT_GROUP => Some(&self.default.upload),
      LEECHERS_GROUP => Some(&self.leechers.upload),
      BUDDIES_GROUP => Some(&self.buddies.upload),
      name => Some(&self.user_defined.get(name)?.upload),
    }
  }
}

/// Our buddies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Buddies {
  /// Upload queue and limits of our buddies. Their slots and speed limit are always set and
  /// start at no limit, so buddies have a queue of their own served before everyone else and a
  /// buddy who shares too little is not slowed by the leechers' settings.
  pub upload: GroupUpload,
}

impl Default for Buddies {
  fn default() -> Self {
    Self {
      upload: GroupUpload {
        slots: Some(0),
        speed_limit: Some(0),
        ..GroupUpload::default()
      },
    }
  }
}

/// Uploads of a built-in group.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Group {
  /// Upload queue and limits of the group.
  pub upload: GroupUpload,
}

/// Upload queue and limits of a group. A setting left unset is taken from the next group down
/// the order the user is in that sets it, and for `default` from the global settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct GroupUpload {
  /// How the group's own queue is served: `roundrobin`, one file per user in turn, or
  /// `firstinfirstout`, in the order queued. Setting it or `slots` gives the group a queue of
  /// its own.
  pub strategy: Option<QueueStrategy>,
  /// Maximum number of uploads of the group's own queue running at once, shared by its
  /// members. `0` for every slot.
  pub slots: Option<u32>,
  /// Total upload speed limit of the group's members together in KiB/s. `0` for no limit.
  pub speed_limit: Option<u32>,
  /// Per-user limits of the group.
  pub limits: Limits,
}

impl GroupUpload {
  /// Whether the group sets any upload setting.
  pub fn sets_any(&self) -> bool {
    *self != Self::default()
  }

  /// Whether the group has a queue of its own.
  pub fn own_queue(&self) -> bool {
    self.slots.is_some() || self.strategy.is_some()
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
  /// Color the group is shown in.
  pub color: GroupColor,
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
