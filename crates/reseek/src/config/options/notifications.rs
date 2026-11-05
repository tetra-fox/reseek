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
  /// Suppresses notifications of every type while the tab is visible and, for a message, its
  /// chat is open.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_ONLY_WHEN_HIDDEN")]
  pub only_when_hidden: bool,
  /// How a private message is announced.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_PRIVATE_MESSAGES")]
  pub private_messages: NotificationMode,
  /// How a room message that mentions this account is announced.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_MENTIONS")]
  pub mentions: NotificationMode,
  /// How each message in a room set to notify of all messages is announced.
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
}

/// Buddy notifications, under the same browser permission and tab visibility rules as chat
/// notifications. Each type is `off`, or announced by `sound`, by a `desktop` notification, or by
/// `both`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct BuddyNotifications {
  /// How a buddy coming online, going away or going offline is announced, for the buddies set
  /// to tell of it.
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
  /// How the end of a download job is announced: everything queued in one go, such as a file,
  /// a folder or an album, told of once when all of it has finished.
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
