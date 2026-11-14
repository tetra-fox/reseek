//! The event stream: every frame `GET /live` sends, as one enum whose `type` is the SSE
//! `event:` line and whose `data` is the `data:` line.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::activity::ActivityEntry;
use crate::analysis::{AnalysisCompleted, AnalysisJob};
use crate::auth::PrincipalSummary;
use crate::events::{BuddyPresenceChanged, EventRecord};
use crate::jobs::{Job, JobSchedule, JobsRemoved};
use crate::logs::LogRecord;
use crate::messaging::conversations::{Conversation, ConversationMessage, ConversationUpdate};
use crate::messaging::rooms::{
  JoinedRoom, PublicRoomMessage, Room, RoomHistoryGap, RoomMessageReceived, RoomUserJoined,
  RoomUserLeft, RoomUserStatus,
};
use crate::messaging::{ChatPins, ChatUnread, ServerMessage};
use crate::options::OptionsDocument;
use crate::search::{Search, SearchResponseReceived, WishlistFound};
use crate::state::{State, VersionInfo};
use crate::telemetry::TransferMetrics;
use crate::transfers::{DownloadJobFinished, DownloadQueue, Transfer, TransferRemoved};
use crate::users::{BrowseProgress, UserRemoved, UserSummary};

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
  /// a wishlist search found files it had not found before; also on the `application` topic,
  /// so every open page can tell of it
  #[serde(rename = "search.wishlist_found")]
  SearchWishlistFound(WishlistFound),
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
  /// the order of the downloads waiting in the local queue changed
  #[serde(rename = "transfers.queue")]
  TransfersQueue(DownloadQueue),
  /// every download queued in one go has ended; sent once per job
  #[serde(rename = "transfers.job_finished")]
  TransfersJobFinished(DownloadJobFinished),
  /// a job was added or changed
  #[serde(rename = "jobs.updated")]
  JobsUpdated(Box<Job>),
  /// jobs were taken out of the list
  #[serde(rename = "jobs.removed")]
  JobsRemoved(JobsRemoved),
  /// every schedule, on open and whenever one runs or its option changes
  #[serde(rename = "jobs.schedules")]
  JobsSchedules(Vec<JobSchedule>),
  /// a file was queued for audio analysis, by anyone
  #[serde(rename = "files.analysis_queued")]
  FilesAnalysisQueued(AnalysisJob),
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
  /// a room's history has a gap, from a time reseek was not connected
  #[serde(rename = "rooms.gap")]
  RoomsGap(RoomHistoryGap),
  /// a message in any public room, while a stream follows the `public_rooms` topic; not stored
  #[serde(rename = "rooms.public_message")]
  RoomsPublicMessage(PublicRoomMessage),
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
  /// a buddy we asked to be told about came online, went away or went offline; also on the
  /// `application` topic, so every open page can tell of it
  #[serde(rename = "users.presence")]
  UsersPresence(BuddyPresenceChanged),
  /// unread counts, on open and on change
  #[serde(rename = "chat.unread")]
  ChatUnread(ChatUnread),
  /// the pinned chats in the user's order with their revision, on open and on change
  #[serde(rename = "chat.pins")]
  ChatPins(ChatPins),
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
  /// `search:<id>`, `transfers`, `jobs`, `conversations`, `rooms`, `users`, `events`, `activity`;
  /// `application` when left out
  #[param(example = "application,transfers")]
  pub topics: Option<String>,
  /// an API token or a configuration API key, for clients that cannot set an
  /// `Authorization` header; never a session cookie's value
  pub access_token: Option<String>,
  /// this stream follows the close of another of the client's, whose place it may need: at the
  /// stream cap it waits up to 2 s for the daemon to see that close rather than being refused
  #[serde(default)]
  pub reopen: bool,
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
  /// this run of the daemon, new each time it starts. Every frame's id carries it, and a stream
  /// resumes only from a frame of the same run, so a store that saw another run reads again
  pub epoch: String,
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
