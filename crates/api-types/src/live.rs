//! The event stream: every frame `GET /live` sends, as one enum whose `type` is the SSE
//! `event:` line and whose `data` is the `data:` line.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::activity::ActivityEntry;
use crate::analysis::AnalysisCompleted;
use crate::auth::PrincipalSummary;
use crate::events::EventRecord;
use crate::logs::LogRecord;
use crate::messaging::conversations::{Conversation, ConversationMessage, ConversationUpdate};
use crate::messaging::rooms::{
  HistoryGap, JoinedRoom, Room, RoomMessageReceived, RoomUserJoined, RoomUserLeft, RoomUserStatus,
};
use crate::messaging::{ChatUnread, ServerMessage};
use crate::options::OptionsDocument;
use crate::search::{Search, SearchResponseReceived};
use crate::state::{State, VersionInfo};
use crate::telemetry::TransferMetrics;
use crate::transfers::{Transfer, TransferRemoved};
use crate::users::{BrowseProgress, UserRemoved, UserStatusChanged, UserSummary};

/// One frame of the event stream. Frames are named `<topic>.<event>`; the control frames
/// `hello`, `lagged` and `bye` belong to no topic and are always sent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "data")]
pub enum LiveFrame {
  /// the first frame of every stream
  #[serde(rename = "hello")]
  Hello(Hello),
  /// the stream was reopened too late to replay what was missed; lists without a snapshot
  /// must be fetched again
  #[serde(rename = "lagged")]
  Lagged(Lagged),
  /// the server is about to close the stream
  #[serde(rename = "bye")]
  Bye(Bye),
  /// the application state, on open and on every change
  #[serde(rename = "application.state")]
  ApplicationState(Box<State>),
  /// the options in effect, on open and after every change
  #[serde(rename = "application.options")]
  ApplicationOptions(OptionsDocument),
  /// a message from the Soulseek server's administrators
  #[serde(rename = "application.server_message")]
  ApplicationServerMessage(ServerMessage),
  /// the last log records, on open
  #[serde(rename = "logs.buffer")]
  LogsBuffer(Vec<LogRecord>),
  /// a new log record
  #[serde(rename = "logs.record")]
  LogsRecord(LogRecord),
  /// transfer figures, when they change
  #[serde(rename = "metrics.update")]
  MetricsUpdate(TransferMetrics),
  /// every search, on open
  #[serde(rename = "search.list")]
  SearchList(Vec<Search>),
  /// a search was started
  #[serde(rename = "search.create")]
  SearchCreate(Search),
  /// a search changed
  #[serde(rename = "search.update")]
  SearchUpdate(Search),
  /// a search was deleted
  #[serde(rename = "search.delete")]
  SearchDelete(Search),
  /// a response arrived for the search the topic names
  #[serde(rename = "search.response")]
  SearchResponse(SearchResponseReceived),
  /// a transfer was added
  #[serde(rename = "transfers.added")]
  TransfersAdded(Box<Transfer>),
  /// a transfer changed
  #[serde(rename = "transfers.updated")]
  TransfersUpdated(Box<Transfer>),
  /// a transfer was removed
  #[serde(rename = "transfers.removed")]
  TransfersRemoved(TransferRemoved),
  /// a file's audio analysis finished
  #[serde(rename = "files.analysis")]
  FilesAnalysis(AnalysisCompleted),
  /// every conversation, on open
  #[serde(rename = "conversations.list")]
  ConversationsList(Vec<Conversation>),
  /// a private message was sent or received
  #[serde(rename = "conversations.message")]
  ConversationsMessage(ConversationMessage),
  /// a conversation changed
  #[serde(rename = "conversations.update")]
  ConversationsUpdate(ConversationUpdate),
  /// every joined room, on open
  #[serde(rename = "rooms.list")]
  RoomsList(Vec<JoinedRoom>),
  /// a joined room changed
  #[serde(rename = "rooms.update")]
  RoomsUpdate(Box<Room>),
  /// a message in a joined room
  #[serde(rename = "rooms.message")]
  RoomsMessage(RoomMessageReceived),
  /// a user joined a room we are in
  #[serde(rename = "rooms.user_joined")]
  RoomsUserJoined(RoomUserJoined),
  /// a user left a room we are in
  #[serde(rename = "rooms.user_left")]
  RoomsUserLeft(RoomUserLeft),
  /// a member of a room we are in went away or came back
  #[serde(rename = "rooms.user_status")]
  RoomsUserStatus(RoomUserStatus),
  /// room history has a gap, from a time reseek was not connected
  #[serde(rename = "rooms.gap")]
  RoomsGap(HistoryGap),
  /// a watched user's status changed
  #[serde(rename = "users.status")]
  UsersStatus(UserStatusChanged),
  /// how far a browse of a user's shares has come, at most every 250 ms per user and once
  /// more at 100 %
  #[serde(rename = "users.browse_progress")]
  UsersBrowseProgress(BrowseProgress),
  /// a user in the directory changed
  #[serde(rename = "users.updated")]
  UsersUpdated(Box<UserSummary>),
  /// a user was removed from the directory
  #[serde(rename = "users.removed")]
  UsersRemoved(UserRemoved),
  /// unread counts, on open and on change
  #[serde(rename = "chat.unread")]
  ChatUnread(ChatUnread),
  /// an event was raised
  #[serde(rename = "events.raised")]
  EventsRaised(Box<EventRecord>),
  /// the activity log's entries, oldest first, on open
  #[serde(rename = "activity.buffer")]
  ActivityBuffer(Vec<ActivityEntry>),
  /// a new activity entry, or one that grew
  #[serde(rename = "activity.entry")]
  ActivityEntry(ActivityEntry),
}

/// The query of `GET /live`, which opens the event stream.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LiveQuery {
  /// the topics to follow, comma-separated: `application`, `logs`, `metrics`, `search`,
  /// `search:<id>`, `transfers`, `conversations`, `rooms`, `users`, `events`, `activity`;
  /// `application` when left out
  #[param(example = "application,transfers")]
  pub topics: Option<String>,
  /// an API token or a configuration API key, for clients that can not set an
  /// `Authorization` header; never a session cookie's value
  pub access_token: Option<String>,
}

/// The first frame of every stream.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
  /// the server's clock when the stream opened
  pub server_time: DateTime<Utc>,
  /// the running version
  pub version: VersionInfo,
  /// who the stream is for
  pub principal: PrincipalSummary,
  /// the topics the stream carries, in the order asked for
  pub topics: Vec<String>,
  /// whether the frames missed since `Last-Event-ID` follow instead of fresh snapshots
  pub resumed: bool,
}

/// The data of a `lagged` frame, an empty object.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Lagged {}

/// Why the server closes a stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ByeReason {
  /// the credential stopped being valid; reconnecting does not help, logging in does
  Unauthorized,
  /// the stream fell too far behind; reconnect with `Last-Event-ID`
  Lagged,
  /// the daemon is shutting down
  Shutdown,
}

/// The data of a `bye` frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bye {
  /// why the stream closes
  pub reason: ByeReason,
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  /// Every frame's name, as the event stream's catalog spells it.
  const CATALOG: &[&str] = &[
    "hello",
    "lagged",
    "bye",
    "application.state",
    "application.options",
    "application.server_message",
    "logs.buffer",
    "logs.record",
    "metrics.update",
    "search.list",
    "search.create",
    "search.update",
    "search.delete",
    "search.response",
    "transfers.added",
    "transfers.updated",
    "transfers.removed",
    "files.analysis",
    "conversations.list",
    "conversations.message",
    "conversations.update",
    "rooms.list",
    "rooms.update",
    "rooms.message",
    "rooms.user_joined",
    "rooms.user_left",
    "rooms.user_status",
    "rooms.gap",
    "users.status",
    "users.browse_progress",
    "users.updated",
    "users.removed",
    "chat.unread",
    "events.raised",
    "activity.buffer",
    "activity.entry",
  ];

  #[test]
  fn every_frame_is_named_as_the_catalog_spells_it() {
    // reading an unknown type lists every name the enum takes, in declaration order
    let e = serde_json::from_value::<LiveFrame>(json!({ "type": "nope", "data": null }))
      .unwrap_err()
      .to_string();
    let listed = e
      .split("expected one of ")
      .nth(1)
      .unwrap_or_else(|| panic!("{e}"));
    let names: Vec<&str> = listed
      .split(", ")
      .map(|n| n.trim_matches(|c: char| c == '`' || !c.is_ascii_graphic()))
      .map(|n| n.split('`').next().unwrap_or(n))
      .collect();
    assert_eq!(names, CATALOG);
  }
}
