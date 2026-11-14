//! The data of each event type.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::EventType;
use crate::analysis::AudioAnalysis;
use crate::auth::{Role, Via};
use crate::jobs::{JobCounts, JobEnding, JobKind, JobStarter};
use crate::messaging::conversations::PrivateMessage;
use crate::messaging::rooms::RoomMessage;
use crate::transfers::Transfer;
use crate::users::UserPresence;
use crate::users::blocking::BlockScope;

/// The data of an event that has no fields of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoFields {
  /// the shape version of this event type, 0
  pub version: u32,
}

/// The data of `DownloadFileComplete`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadFileComplete {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the absolute path the file was moved to
  pub local_filename: String,
  /// the remote filename as requested, with the peer's separators
  pub remote_filename: String,
  /// the finished transfer
  pub transfer: Transfer,
}

/// The data of `DownloadDirectoryComplete`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadDirectoryComplete {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the absolute local directory the files were moved into
  pub local_directory_name: String,
  /// the remote directory, the remote filename without its last segment; empty for a file
  /// without a directory
  pub remote_directory_name: String,
  /// the user the directory came from
  pub username: String,
}

/// Where a download failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DownloadFailureStage {
  /// the transfer itself failed
  Download,
  /// every byte arrived but the file could not be moved into the downloads directory
  Move,
}

/// The data of `DownloadFileFailed`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadFileFailed {
  /// the shape version of this event type, 0
  pub version: u32,
  /// where it failed
  pub stage: DownloadFailureStage,
  /// the transfer's error: the peer's refusal, the timeout, the I/O or move error
  #[schema(example = "File not shared.")]
  pub reason: String,
  /// the incomplete file holding the bytes that arrived, null when no file was created
  pub local_filename: Option<String>,
  /// the remote filename as requested, with the peer's separators
  pub remote_filename: String,
  /// the finished transfer
  pub transfer: Transfer,
}

/// The data of `UploadFileComplete`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadFileComplete {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the absolute local path of the file that was sent
  pub local_filename: String,
  /// the filename as the peer asked for it
  pub remote_filename: String,
  /// the finished transfer
  pub transfer: Transfer,
}

/// The data of `PrivateMessageReceived`, raised again at each login until the message is read;
/// a repeat carries the first one's `localId`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrivateMessageReceived {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the message as stored
  pub message: PrivateMessage,
}

/// The data of `RoomMessageReceived`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoomMessageReceivedEvent {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the message as stored
  pub message: RoomMessage,
}

/// The data of `UserBlocked`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserBlocked {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the blocked user
  pub username: String,
  /// why, in the words of who blocked them; null when none was given
  pub reason: Option<String>,
  /// what the block stops
  pub scopes: Vec<BlockScope>,
  /// when the block ends on its own, null for never
  pub expires_at: Option<DateTime<Utc>>,
  /// the subject of the principal that blocked the user
  #[schema(example = "local:admin")]
  pub by: String,
}

/// Why a block ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum UnblockReason {
  /// someone removed it
  Manual,
  /// its expiry passed
  Expired,
}

/// The data of `UserUnblocked`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserUnblocked {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the user who is no longer blocked
  pub username: String,
  /// why the block ended
  pub reason: UnblockReason,
  /// the subject of the principal that removed the block, null when it expired
  pub by: Option<String>,
}

/// The data of `TokenCreated` and `TokenRevoked`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TokenChanged {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the token's id
  pub token_id: Uuid,
  /// the token's name
  pub name: String,
  /// what a request with the token may do
  pub role: Role,
  /// the subject of the principal that created or revoked it
  #[schema(example = "local:admin")]
  pub by: String,
}

/// The data of `SessionCreated`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreated {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the stable id of who signed in
  #[schema(example = "local:admin")]
  pub subject: String,
  /// the name to show for who signed in
  pub name: String,
  /// how they signed in
  pub via: Via,
  /// the role they signed in with; each request reads it again from the configuration
  pub role: Role,
  /// the client address they signed in from, null when unknown
  pub ip: Option<String>,
}

/// Why a session ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SessionRevokeReason {
  /// its owner signed out
  Logout,
  /// an administrator or its owner revoked it from the sessions list
  Admin,
  /// the identity provider ended it
  Backchannel,
  /// the reverse proxy forwards another user or role than the session's
  Proxy,
  /// it expired
  Expired,
}

/// The data of `SessionRevoked`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionRevoked {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the session's id, as the sessions list shows it
  pub session_id: String,
  /// the stable id of who the session belonged to
  #[schema(example = "local:admin")]
  pub subject: String,
  /// the subject of the principal that ended it, null when it expired or the identity provider
  /// or the reverse proxy ended it
  pub by: Option<String>,
  /// why it ended
  pub reason: SessionRevokeReason,
}

/// The data of `LoginFailed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginFailed {
  /// the shape version of this event type, 0
  pub version: u32,
  /// `local` for a password login, else the OpenID Connect provider's configured name
  #[schema(example = "local")]
  pub provider: String,
  /// the client address the attempt came from
  pub ip: String,
  /// why it was refused, without the credentials presented
  pub reason: String,
}

/// The data of `FileAnalysisComplete`: the report without its per-window measures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileAnalysisComplete {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the report, with `windows` null
  #[serde(flatten)]
  pub report: Box<AudioAnalysis>,
}

/// The data of `ScriptRunCompleted`: how one run of a script went.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRunCompleted {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the script's name in the configuration
  #[schema(example = "notify")]
  pub script: String,
  /// the run's id, which the script's log lines carry
  pub run_id: Uuid,
  /// the id of the event the script ran for
  pub event_id: Uuid,
  /// the type of the event the script ran for
  pub event_type: EventType,
  /// the script exited with code 0 within its timeout
  pub succeeded: bool,
  /// the exit code, null when the process was killed or never started
  pub exit_code: Option<i32>,
  /// the script ran past its timeout and was killed
  pub timed_out: bool,
  /// why it failed when no exit code says so, such as a spawn error or the timeout; null
  /// otherwise
  pub error: Option<String>,
  /// how long it ran, in milliseconds
  pub duration_ms: u64,
  /// the last 20 lines of its output and error streams as read, at most 4 KiB
  pub output: String,
}

/// An error, as an event shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventError {
  /// the short name of the error's type
  #[serde(rename = "type")]
  #[schema(example = "ConnectionReset")]
  pub kind: String,
  /// its message
  pub message: String,
}

/// The data of `SoulseekClientDisconnected`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SoulseekClientDisconnected {
  /// the shape version of this event type, 0
  pub version: u32,
  /// why the connection ended, as the client reports it
  #[schema(example = "disconnected by the user")]
  pub message: String,
  /// the error that ended it, null when it ended without one
  pub exception: Option<EventError>,
}

/// The data of `RelayAgentConnected`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelayAgentConnected {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the agent's instance name
  pub agent: String,
  /// the address it connected from
  pub ip_address: String,
}

/// The data of `RelayAgentDisconnected`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelayAgentDisconnected {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the agent's instance name
  pub agent: String,
  /// why the socket closed
  #[schema(example = "superseded")]
  pub reason: String,
}

/// The data of `RelayAgentSharesUpdated`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelayAgentSharesUpdated {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the agent's instance name
  pub agent: String,
  /// files over the agent's shares
  pub files: u32,
  /// directories over the agent's shares
  pub directories: u32,
}

/// The data of `BuddyPresenceChanged`, and of the `users.presence` frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuddyPresenceChanged {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the buddy
  pub username: String,
  /// their presence now
  pub presence: UserPresence,
  /// their presence before
  pub previous: UserPresence,
}

/// The data of `JobFinished`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobFinished {
  /// the shape version of this event type, 0
  pub version: u32,
  /// the job's id
  pub id: Uuid,
  /// what it did
  pub kind: JobKind,
  /// what it worked on
  #[schema(example = "Album from alice")]
  pub name: String,
  /// who or what started it
  pub starter: JobStarter,
  /// how it ended
  pub ending: JobEnding,
  /// why it failed, when the job itself did
  pub error: Option<String>,
  /// what it came to in words
  pub summary: Option<String>,
  /// how many parts it had and how they ended
  pub counts: JobCounts,
}
