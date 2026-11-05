//! How long finished things are kept. A duration of `0` turns its limit off; a duration that
//! is not set keeps things forever.

use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

/// How often the retention job deletes what is past the retention options.
pub const RETENTION_SWEEP: Duration = Duration::from_secs(300);

/// Retention of finished items.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Retention {
  /// Age after which finished searches are deleted. Minimum: 5 minutes.
  #[serde(with = "crate::config::duration")]
  pub search: Option<Duration>,
  /// Finished transfers.
  pub transfers: TransfersRetention,
  /// Files in the download directories.
  pub files: FileRetention,
  /// Age after which daily log files are deleted. Minimum: 2 days.
  #[cascade(apply = "restart")]
  #[serde(with = "crate::config::duration")]
  pub logs: Duration,
  /// Chat history.
  pub messages: MessageRetention,
  /// Time without being seen after which a user is removed from the users directory. Buddies
  /// and noted, tagged, blocked and grouped users are kept.
  #[cascade(env = "RESEEK_RETENTION_USERS")]
  #[serde(with = "crate::config::duration")]
  pub users: Option<Duration>,
  /// Age after which logged events are deleted. `0` keeps them.
  #[cascade(env = "RESEEK_RETENTION_EVENTS")]
  #[serde(with = "crate::config::duration")]
  pub events: Duration,
  /// Maximum number of logged events kept. The oldest beyond it are deleted. `0` keeps all of
  /// them.
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

/// Retention of finished transfers by direction, and of removed transfers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct TransfersRetention {
  /// Finished uploads.
  pub upload: TransferRetention,
  /// Finished downloads.
  pub download: TransferRetention,
  /// Time after removal from the transfer lists at which a transfer is deleted permanently.
  /// Minimum: 5 minutes. Removed transfers are kept for the history when not set.
  #[serde(with = "crate::config::duration")]
  pub purge_removed_after: Option<Duration>,
}

/// Ages after which finished transfers are removed, each at least five minutes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct TransferRetention {
  /// Age after which succeeded transfers are removed. Minimum: 5 minutes.
  #[serde(with = "crate::config::duration")]
  pub succeeded: Option<Duration>,
  /// Age after which transfers that ended in an error, a timeout, a rejection or a size
  /// mismatch are removed. Minimum: 5 minutes.
  #[serde(with = "crate::config::duration")]
  pub errored: Option<Duration>,
  /// Age after which canceled transfers are removed. Minimum: 5 minutes.
  #[serde(with = "crate::config::duration")]
  pub canceled: Option<Duration>,
  /// Age after which errored or canceled transfers are removed. Minimum: 5 minutes.
  #[serde(with = "crate::config::duration")]
  pub failed: Option<Duration>,
}

/// Ages after which files in the download directories are deleted, by last write time, each at
/// least 30 minutes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct FileRetention {
  /// Age after which files in the downloads directory are deleted, by last write time.
  /// Minimum: 30 minutes.
  #[serde(with = "crate::config::duration")]
  pub complete: Option<Duration>,
  /// Age after which files in the incomplete directory are deleted, by last write time.
  /// Minimum: 30 minutes.
  #[serde(with = "crate::config::duration")]
  pub incomplete: Option<Duration>,
}

/// Chat history. A message is deleted when it passes its age or count limit, whichever comes
/// first. `0` disables a limit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct MessageRetention {
  /// Age after which read private messages are deleted. `0` for no limit.
  #[cascade(env = "RESEEK_RETENTION_MESSAGES_PRIVATE")]
  #[serde(with = "crate::config::duration")]
  pub private: Duration,
  /// Maximum number of read private messages kept per conversation. Minimum: 250, or `0` for
  /// no limit.
  #[cascade(env = "RESEEK_RETENTION_MESSAGES_MAX_PER_CONVERSATION")]
  pub max_per_conversation: u32,
  /// Age after which room messages are deleted, except those that mention this account.
  /// Minimum: 1 hour, or `0` for no limit.
  #[cascade(env = "RESEEK_RETENTION_MESSAGES_ROOMS")]
  #[serde(with = "crate::config::duration")]
  pub rooms: Duration,
  /// Age after which room messages that mention this account are deleted. `0` for no limit.
  #[cascade(env = "RESEEK_RETENTION_MESSAGES_MENTIONS")]
  #[serde(with = "crate::config::duration")]
  pub mentions: Duration,
  /// Maximum number of messages kept per room. Minimum: 250, or `0` for no limit.
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
