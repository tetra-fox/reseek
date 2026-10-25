//! Chat rooms: joined rooms, their members, messages and stored history, the server's room
//! list, the push frames about rooms, and the bodies and queries of the room calls.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::chat::NotifyLevel;
use crate::state::UserStatistics;
use crate::users::UserPresence;

/// A member of a room we are in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomUser {
  /// the member
  #[schema(example = "alice")]
  pub username: String,
  /// the member's presence
  pub status: UserPresence,
  /// the member's statistics as the room messages carry them
  #[serde(flatten)]
  pub statistics: UserStatistics,
  /// every upload slot of the member is busy; null when the server did not say
  pub slots_full: Option<bool>,
  /// the member's country as ISO 3166-1 alpha-2; null while the member is offline or when the
  /// server sent a blank
  #[schema(example = "DE")]
  pub country_code: Option<String>,
  /// the member is us
  #[serde(rename = "self")]
  pub is_self: bool,
}

/// A user's ticker in a room, the short status line other clients show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomTicker {
  /// the user
  pub username: String,
  /// the ticker's text
  pub ticker: String,
}

/// One room message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomMessage {
  /// the message's id in the stored history, increasing in the order messages arrived
  pub local_id: i64,
  /// when it arrived; the server sends no time for room messages
  pub timestamp: DateTime<Utc>,
  /// the author
  #[schema(example = "alice")]
  pub username: String,
  /// the text
  pub message: String,
  /// the room
  pub room_name: String,
  /// the author is us
  #[serde(rename = "self")]
  pub is_self: bool,
  /// the message mentions our username
  pub is_mention: bool,
  /// the author is blocked for rooms, so the message is shown collapsed
  pub hidden: bool,
}

/// A room we are in, as `GET /rooms/joined/{roomName}` returns it and `rooms.update` frames
/// carry it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Room {
  /// the room's name
  #[schema(example = "indie")]
  pub name: String,
  /// a private room, which only members can join
  pub is_private: bool,
  /// the owner of a private room, null for a public one
  pub owner: Option<String>,
  /// the operators of a private room, null for a public one
  pub operators: Option<Vec<String>>,
  /// how many operators a private room has, null for a public one
  pub operator_count: Option<u32>,
  /// the members of a private room, the owner included; null for a public one
  pub members: Option<Vec<String>>,
  /// who is in the room now
  pub users: Vec<RoomUser>,
  /// the members' tickers
  pub tickers: Vec<RoomTicker>,
  /// the latest messages, oldest first; null in frames
  pub messages: Option<Vec<RoomMessage>>,
}

/// A room we are in, as `GET /rooms/joined` lists it and `rooms.list` frames carry it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JoinedRoom {
  /// the room's name
  #[schema(example = "indie")]
  pub name: String,
  /// messages after the read marker, not ours and not hidden
  pub unread: u32,
  /// those of them that mention us
  pub mentions: u32,
  /// which messages notify
  pub notify: NotifyLevel,
  /// who is in the room now
  pub user_count: u32,
  /// when the latest stored message arrived, null when none is stored
  pub last_message_at: Option<DateTime<Utc>>,
}

/// A room from the server's room list, as `GET /rooms/available` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomInfo {
  /// the room's name
  pub name: String,
  /// how many users are in it, as the server counts them
  pub user_count: u32,
  /// a private room we are a member of
  pub is_private: bool,
  /// a private room we own
  pub is_owned: bool,
  /// a private room we are an operator of
  pub is_moderated: bool,
  /// we are in the room
  pub joined: bool,
}

/// Why reseek missed a room's messages for a while.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum GapReason {
  /// the daemon ran but was not connected to the Soulseek server
  Disconnected,
  /// the daemon was not running
  DaemonDown,
}

/// An interval with no room history because reseek was not there to receive messages. The
/// data of a `rooms.gap` frame, which is sent once logged in again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryGap {
  /// when reseek stopped receiving
  pub from: DateTime<Utc>,
  /// when it received again, null while it is still disconnected
  pub to: Option<DateTime<Utc>>,
  /// why
  pub reason: GapReason,
}

fn default_history_limit() -> u32 {
  100
}

/// The query of a room's message history, `GET /rooms/joined/{roomName}/messages` and
/// `GET /rooms/history/{roomName}/messages`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct RoomMessagesQuery {
  /// only messages before the one with this `localId`, the newest when left out
  pub before: Option<i64>,
  /// how many messages to return, at most 500
  #[serde(default = "default_history_limit")]
  #[param(default = 100, maximum = 500)]
  pub limit: u32,
  /// also return the messages of users blocked for rooms
  #[serde(default)]
  pub include_hidden: bool,
}

/// One page of a room's message history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomHistory {
  /// the messages, oldest first
  pub messages: Vec<RoomMessage>,
  /// older messages exist
  pub has_more: bool,
  /// the intervals without history between the page's first message and its last, or now on
  /// the newest page, oldest first
  pub gaps: Vec<HistoryGap>,
}

/// The query of `GET /rooms/history`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct RoomHistoryListQuery {
  /// only this room, every room with stored messages when left out
  pub room: Option<String>,
}

/// Every room with stored messages, the reply of `GET /rooms/history`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomHistoryList {
  /// the rooms
  pub rooms: Vec<StoredRoom>,
}

/// A room with stored messages, joined now or not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoredRoom {
  /// the room's name
  pub name: String,
  /// how many messages are stored
  pub message_count: u64,
  /// when the oldest stored message arrived
  pub first_at: DateTime<Utc>,
  /// when the newest stored message arrived
  pub last_at: DateTime<Utc>,
  /// we are in the room now
  pub joined: bool,
}

/// A room to join, the body of `POST /rooms/joined`. Joining a room that does not exist creates
/// it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JoinRoom {
  /// the room's name: 1 to 24 printable ASCII characters, no space at either end and no two
  /// spaces in a row
  #[schema(example = "indie")]
  pub name: String,
  /// create the room as a private one when it does not exist yet; ignored for a room that does
  #[serde(default)]
  pub private: bool,
}

/// Our ticker in a room, the body of `POST /rooms/joined/{roomName}/ticker`, written as a
/// bare JSON string; an empty string clears it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(example = "listening to Animals")]
pub struct TickerText(pub String);

/// A user to add to a private room as a member or operator, the body of
/// `POST /rooms/joined/{roomName}/members` and `POST /rooms/joined/{roomName}/operators`,
/// written as a bare JSON string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(example = "alice")]
pub struct RoomMemberName(pub String);

/// Moves a room's read marker, the body of `PUT /rooms/joined/{roomName}/read`. The marker
/// never moves back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadMarker {
  /// the `localId` of the newest message shown as read
  pub local_id: i64,
}

/// Names a room by query parameter instead of by path segment, for names a path can not
/// carry. When present it wins over the path's room name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct RoomNameQuery {
  /// the room's name, exactly
  pub room_name: Option<String>,
}

/// A message arrived in a room we are in, the data of a `rooms.message` frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomMessageReceived {
  /// the room
  pub room_name: String,
  /// the message
  pub message: RoomMessage,
}

/// A user joined a room we are in, the data of a `rooms.user_joined` frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomUserJoined {
  /// the room
  pub room_name: String,
  /// the user who joined
  pub user: RoomUser,
}

/// A user left a room we are in, the data of a `rooms.user_left` frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomUserLeft {
  /// the room
  pub room_name: String,
  /// the user who left
  pub username: String,
}
