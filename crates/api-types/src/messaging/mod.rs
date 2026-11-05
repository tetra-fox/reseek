//! Chat, in private conversations and in rooms. This module holds what the two share: message
//! bodies, notification levels, unread counts, the message search, exports, and the server's
//! own messages.

pub mod conversations;
pub mod rooms;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// The text of a message to send, the body of `POST /conversations/{username}` and
/// `POST /rooms/joined/{roomName}/messages`, written as a bare JSON string. It must not be
/// empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(example = "hello")]
pub struct MessageText(pub String);

/// Which messages of a room or conversation notify.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum NotifyLevel {
  /// every message
  All,
  /// only messages that mention us
  Mentions,
  /// none
  None,
}

/// Sets which messages of a room or conversation notify, the body of
/// `PUT /rooms/joined/{roomName}/notify` and `PUT /conversations/{username}/notify`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotifySetting {
  /// the level
  pub notify: NotifyLevel,
}

/// Whether a chat is a room or a private conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChatKind {
  /// a chat room
  Room,
  /// a private conversation
  User,
}

/// A chat pinned above the others in the chat list. A list of these in the user's order is the
/// body of `PUT /chat/pins`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatPin {
  /// a room or a conversation
  pub kind: ChatKind,
  /// the room's name or the other user's name
  pub name: String,
}

/// The pinned chats in the user's order and the revision of that list, which grows by one with
/// every change. `GET /chat/pins` answers it, `PUT /chat/pins` answers it with the list it set,
/// and it is the data of a `chat.pins` frame. Answers and frames can arrive in any order, so a
/// client keeps the list of the highest revision it has seen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatPins {
  /// the number of changes the list has had
  pub revision: i64,
  /// the pinned chats, the first on top
  pub pins: Vec<ChatPin>,
}

/// Unread counts over every conversation and room, the data of a `chat.unread` frame.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatUnread {
  /// unacknowledged inbound messages, by peer
  pub conversations: BTreeMap<String, u32>,
  /// unread messages and mentions, by room
  pub rooms: BTreeMap<String, RoomUnread>,
  /// the conversations' unread messages plus the rooms' unread mentions; plain room messages
  /// do not count
  pub total: u32,
  /// the rooms' unread mentions
  pub total_mentions: u32,
}

/// A room's unread counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomUnread {
  /// messages after the read marker, not ours and not hidden
  pub unread: u32,
  /// those of them that mention us
  pub mentions: u32,
}

/// Which stored messages a message search looks at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MessageSearchKind {
  /// private and room messages
  #[default]
  All,
  /// private messages
  Private,
  /// room messages
  Rooms,
}

fn default_search_limit() -> u32 {
  50
}

/// The query of `GET /messages/search`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct MessageSearchQuery {
  /// a full-text query, at most 200 characters
  #[param(example = "\"pink floyd\" OR animals")]
  pub q: String,
  /// which messages to search
  #[serde(default)]
  pub kind: MessageSearchKind,
  /// only messages in this room
  pub room: Option<String>,
  /// only messages by or with this user
  pub username: Option<String>,
  /// how many hits to skip
  #[serde(default)]
  pub offset: u32,
  /// how many hits to return
  #[serde(default = "default_search_limit")]
  #[param(default = 50)]
  pub limit: u32,
}

/// Where a message search hit was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MessageHitKind {
  /// a private message
  Private,
  /// a room message
  Room,
}

/// One stored message matching a message search.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessageHit {
  /// where the message is
  pub kind: MessageHitKind,
  /// the room, null for a private message
  pub room: Option<String>,
  /// the author of a room message, the peer of a private message
  pub username: String,
  /// when the message was sent or received
  pub timestamp: DateTime<Utc>,
  /// the text
  pub message: String,
  /// the message's id in its room's history or its conversation
  pub local_id: i64,
  /// the matching part of the text, HTML-escaped, with the matches inside `<mark>` and
  /// `</mark>`
  pub snippet: String,
}

/// One page of message search hits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessageSearchResults {
  /// the hits on this page
  pub hits: Vec<MessageHit>,
  /// how many messages match, on every page together
  pub total: u64,
}

/// What form an export is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
  /// a JSON array
  Json,
  /// plain text, one entry per line
  Text,
}

/// The query of the export calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ExportQuery {
  /// the form to write
  pub format: ExportFormat,
}

/// A message from the Soulseek server itself, a broadcast to every user or a notice to us,
/// the data of an `application.server_message` frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerMessage {
  /// the text
  pub message: String,
  /// when it arrived
  pub received_at: DateTime<Utc>,
}
