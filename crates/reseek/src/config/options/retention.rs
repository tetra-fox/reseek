//! How long finished things are kept. A duration of `0` turns its limit off; a duration that
//! is not set keeps things forever.

use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

/// How often the retention job deletes what is past the retention options.
pub const RETENTION_SWEEP: Duration = Duration::from_secs(300);

/// How long finished things are kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Retention {
  /// Finished searches older than this are deleted, at least five minutes.
  #[serde(with = "humantime_serde")]
  pub search: Option<Duration>,
  /// Finished transfers.
  pub transfers: TransfersRetention,
  /// Files in the download directories.
  pub files: FileRetention,
  /// Daily log files older than this are deleted, at least two days.
  #[cascade(apply = "restart")]
  #[serde(with = "humantime_serde")]
  pub logs: Duration,
  /// Chat history.
  pub messages: MessageRetention,
  /// Users not seen for this long leave the users directory, unless pinned, noted, tagged,
  /// blocked or in a group.
  #[cascade(env = "RESEEK_RETENTION_USERS")]
  #[serde(with = "humantime_serde")]
  pub users: Option<Duration>,
  /// Logged events older than this are deleted; `0` keeps them.
  #[cascade(env = "RESEEK_RETENTION_EVENTS")]
  #[serde(with = "humantime_serde")]
  pub events: Duration,
  /// The most logged events kept; the oldest beyond it are deleted, `0` keeps them all.
  #[cascade(env = "RESEEK_RETENTION_EVENTS_MAX_ROWS")]
  pub events_max_rows: u32,
}

impl Default for Retention {
  fn default() -> Self {
    Self {
      search: None,
      transfers: TransfersRetention::default(),
      files: FileRetention::default(),
      logs: Duration::from_secs(14 * 24 * 3600),
      messages: MessageRetention::default(),
      users: None,
      events: Duration::from_secs(30 * 24 * 3600),
      events_max_rows: 100_000,
    }
  }
}

/// Finished transfers, by direction.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct TransfersRetention {
  /// Finished uploads.
  pub upload: TransferRetention,
  /// Finished downloads.
  pub download: TransferRetention,
}

/// Finished transfers older than these are removed, each at least five minutes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct TransferRetention {
  /// Transfers that succeeded.
  #[serde(with = "humantime_serde")]
  pub succeeded: Option<Duration>,
  /// Transfers that failed with an error, a timeout, a refusal or a size mismatch.
  #[serde(with = "humantime_serde")]
  pub errored: Option<Duration>,
  /// Transfers that were cancelled.
  #[serde(with = "humantime_serde")]
  pub cancelled: Option<Duration>,
  /// Transfers that errored or were cancelled.
  #[serde(with = "humantime_serde")]
  pub failed: Option<Duration>,
}

/// Files in the download directories last written this long ago are deleted, each at least 30
/// minutes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct FileRetention {
  /// Files in the downloads directory.
  #[serde(with = "humantime_serde")]
  pub complete: Option<Duration>,
  /// Files in the incomplete directory.
  #[serde(with = "humantime_serde")]
  pub incomplete: Option<Duration>,
}

/// Chat history. A message is deleted when it is past its age or its count limit, whichever
/// comes first; `0` turns a limit off.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct MessageRetention {
  /// Read private messages older than this are deleted.
  #[cascade(env = "RESEEK_RETENTION_MESSAGES_PRIVATE")]
  #[serde(with = "humantime_serde")]
  pub private: Duration,
  /// The most read private messages kept per conversation, at least 250.
  #[cascade(env = "RESEEK_RETENTION_MESSAGES_MAX_PER_CONVERSATION")]
  pub max_per_conversation: u32,
  /// Room messages older than this are deleted, except those that mention us; at least an
  /// hour.
  #[cascade(env = "RESEEK_RETENTION_MESSAGES_ROOMS")]
  #[serde(with = "humantime_serde")]
  pub rooms: Duration,
  /// Room messages that mention us older than this are deleted.
  #[cascade(env = "RESEEK_RETENTION_MESSAGES_MENTIONS")]
  #[serde(with = "humantime_serde")]
  pub mentions: Duration,
  /// The most messages kept per room, at least 250.
  #[cascade(env = "RESEEK_RETENTION_MESSAGES_MAX_PER_ROOM")]
  pub max_per_room: u32,
}

impl Default for MessageRetention {
  fn default() -> Self {
    let days = |n: u64| Duration::from_secs(n * 24 * 3600);
    Self {
      private: days(90),
      max_per_conversation: 100_000,
      rooms: days(90),
      mentions: days(180),
      max_per_room: 100_000,
    }
  }
}
