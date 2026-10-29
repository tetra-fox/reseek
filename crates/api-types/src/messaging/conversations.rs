//! Private conversations: the conversation, its messages, the push frames about them, and the
//! bodies and queries of the conversation calls.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::messaging::NotifyLevel;

/// Which way a private message went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageDirection {
  /// from the peer to us
  In,
  /// from us to the peer
  Out,
}

/// One private message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrivateMessage {
  /// the message's id in this daemon, increasing in the order messages were stored
  pub local_id: i64,
  /// the id the server gave an inbound message, null for an outbound one, which the server
  /// gives no id
  pub id: Option<u32>,
  /// inbound: when the server says it was sent, to the second; outbound: when we sent it
  pub timestamp: DateTime<Utc>,
  /// the peer, whichever way the message went
  #[schema(example = "alice")]
  pub username: String,
  /// which way it went
  pub direction: MessageDirection,
  /// the text, verbatim
  pub message: String,
  /// inbound: the server was told we read it; outbound: always true
  pub is_acknowledged: bool,
  /// inbound: the server delivered it again because it was not acknowledged before; outbound:
  /// always false
  pub was_replayed: bool,
}

/// A private conversation with one peer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
  /// the peer
  #[schema(example = "alice")]
  pub username: String,
  /// false once the conversation was closed; the next message either way opens it again
  pub is_active: bool,
  /// the conversation holds the server's own notices, which can not be answered and never
  /// count as unread
  pub is_server_notice: bool,
  /// inbound messages not acknowledged
  pub un_acknowledged_message_count: u32,
  /// `unAcknowledgedMessageCount` is above zero
  pub has_un_acknowledged_messages: bool,
  /// which messages notify
  pub notify: NotifyLevel,
  /// the unsent text the UI saved for the conversation, empty when none
  pub draft: String,
  /// the newest message either way, null when none is stored
  pub last_message: Option<PrivateMessage>,
  /// the newest messages, oldest first; null in lists and frames and unless asked for
  pub messages: Option<Vec<PrivateMessage>>,
}

/// The state of our conversation with a user, as a user profile shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
  /// the conversation is open
  pub is_active: bool,
  /// inbound messages not acknowledged
  pub un_acknowledged_message_count: u32,
}

/// The query of `GET /conversations`. Conversations are listed by username.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ConversationListQuery {
  /// also list closed conversations
  #[serde(default)]
  pub include_inactive: bool,
  /// list only conversations with unacknowledged messages
  #[serde(default)]
  pub un_acknowledged_only: bool,
  /// also list the conversation holding the server's notices
  #[serde(default)]
  pub include_server: bool,
}

fn default_true() -> bool {
  true
}

/// The query of `GET /conversations/{username}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ConversationQuery {
  /// include the newest 100 messages
  #[serde(default = "default_true")]
  #[param(default = true)]
  pub include_messages: bool,
}

fn default_message_limit() -> u32 {
  100
}

/// The query of `GET /conversations/{username}/messages`, which returns a page of messages
/// oldest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ConversationMessagesQuery {
  /// only unacknowledged messages
  #[serde(default)]
  pub un_acknowledged_only: bool,
  /// only messages stored before the one with this `localId`, the newest when left out
  pub before: Option<i64>,
  /// how many messages to return, at most 1000
  #[serde(default = "default_message_limit")]
  #[param(default = 100, maximum = 1000)]
  pub limit: u32,
}

/// The query of `DELETE /conversations/{username}`, which acknowledges every message and
/// closes the conversation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct CloseConversationQuery {
  /// also delete the stored messages
  #[serde(default)]
  pub purge: bool,
}

/// The unsent text of a conversation, the body of `PUT /conversations/{username}/draft`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDraft {
  /// the text, at most 4 KiB
  pub draft: String,
}

/// A private message was stored, inbound or sent, the data of a `conversations.message`
/// frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
  /// the peer
  pub username: String,
  /// the message
  pub message: PrivateMessage,
}

/// A conversation was opened, acknowledged or closed, the data of a `conversations.update`
/// frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationUpdate {
  /// the conversation, without its messages
  pub conversation: Conversation,
}
