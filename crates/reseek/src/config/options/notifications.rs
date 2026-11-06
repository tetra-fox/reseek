//! Which notifications the web UI raises and how each type is announced.

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::option_enum;

option_enum! {
  /// How a type of notification is announced.
  pub enum NotificationMode {
    /// not announced
    #[default]
    Off = "off",
    /// the notification sound
    Sound = "sound",
    /// a desktop notification, once the browser permits them
    Desktop = "desktop",
    /// the notification sound and a desktop notification
    Both = "both",
  }
}

/// Browser notifications for chat. Each type is `off`, or announced by `sound`, by a `desktop`
/// notification, or by `both`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ChatNotifications {
  /// No notifications while the tab is in view, except for messages in a chat that is not open.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_ONLY_WHEN_HIDDEN")]
  pub only_when_hidden: bool,
  /// How a private message is announced.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_PRIVATE_MESSAGES")]
  pub private_messages: NotificationMode,
  /// How a room message that mentions you is announced.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_MENTIONS")]
  pub mentions: NotificationMode,
  /// How messages in rooms set to notify of every message are announced.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_ROOM_MESSAGES")]
  pub room_messages: NotificationMode,
}

impl Default for ChatNotifications {
  fn default() -> Self {
    Self {
      only_when_hidden: true,
      private_messages: NotificationMode::Both,
      mentions: NotificationMode::Both,
      room_messages: NotificationMode::Off,
    }
  }
}

/// Notifications other than chat.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Notifications {
  /// Transfer notifications.
  pub transfers: TransferNotifications,
  /// Buddy notifications.
  pub buddies: BuddyNotifications,
  /// Notifications of jobs other than downloads.
  pub jobs: JobNotifications,
  /// Search notifications.
  pub search: SearchNotifications,
}

/// Notifications of jobs other than download jobs, which `transfers.job_finished` covers. Each
/// type is `off`, or announced by `sound`, by a `desktop` notification, or by `both`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct JobNotifications {
  /// How the end of a share scan is announced.
  #[cascade(env = "RESEEK_NOTIFICATIONS_JOBS_SCANS")]
  pub scans: NotificationMode,
  /// How the end of an analysis job is announced.
  #[cascade(env = "RESEEK_NOTIFICATIONS_JOBS_ANALYSIS")]
  pub analysis: NotificationMode,
  /// How the end of a cleanup or a database vacuum is announced. A failure is always shown.
  #[cascade(env = "RESEEK_NOTIFICATIONS_JOBS_MAINTENANCE")]
  pub maintenance: NotificationMode,
}

/// Search notifications, under the same browser permission and tab visibility rules as chat
/// notifications.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SearchNotifications {
  /// How new files found by a wishlist search are announced.
  #[cascade(env = "RESEEK_NOTIFICATIONS_SEARCH_WISHLIST")]
  pub wishlist: NotificationMode,
}

impl Default for SearchNotifications {
  fn default() -> Self {
    Self {
      wishlist: NotificationMode::Both,
    }
  }
}

/// Buddy notifications, under the same browser permission and tab visibility rules as chat
/// notifications. Each type is `off`, or announced by `sound`, by a `desktop` notification, or by
/// `both`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct BuddyNotifications {
  /// How a buddy coming online, going away or going offline is announced, for buddies set to
  /// notify.
  #[cascade(env = "RESEEK_NOTIFICATIONS_BUDDIES_PRESENCE")]
  pub presence: NotificationMode,
}

impl Default for BuddyNotifications {
  fn default() -> Self {
    Self {
      presence: NotificationMode::Both,
    }
  }
}

/// Transfer notifications, under the same browser permission and tab visibility rules as chat
/// notifications. Each type is `off`, or announced by `sound`, by a `desktop` notification, or by
/// `both`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct TransferNotifications {
  /// How finished downloads are announced, once per file, folder or album queued together.
  #[cascade(env = "RESEEK_NOTIFICATIONS_TRANSFERS_JOB_FINISHED")]
  pub job_finished: NotificationMode,
  /// How an upload queued by another user is announced.
  #[cascade(env = "RESEEK_NOTIFICATIONS_TRANSFERS_UPLOAD_QUEUED")]
  pub upload_queued: NotificationMode,
}

impl Default for TransferNotifications {
  fn default() -> Self {
    Self {
      job_finished: NotificationMode::Both,
      upload_queued: NotificationMode::Off,
    }
  }
}
