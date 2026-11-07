//! Relay mode: a controller that fronts agents holding shares on other machines. Holds the
//! relay part of the application state, the messages of the socket between an agent and its
//! controller, and the bodies of the relay calls.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::shares::Share;

/// Relay mode and the agents connected to this controller, as part of the application state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelayState {
  /// whether relay mode is on, which makes this daemon a controller its agents connect to
  pub enabled: bool,
  /// the agents connected to this controller, empty while relay mode is off
  pub agents: Vec<RelayAgentState>,
}

/// An agent connected to this controller, listed once it has logged in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelayAgentState {
  /// the agent's instance name
  pub name: String,
  /// the address it connected from
  pub ip_address: String,
  /// when it connected
  pub connected_at: DateTime<Utc>,
  /// whether it finished its handshake and serves files
  pub ready: bool,
  /// when it last sent its share listing
  pub shares_updated_at: Option<DateTime<Utc>>,
  /// the version the agent reported
  pub version: String,
}

/// The version of the relay socket protocol, sent in every envelope and in the handshake.
pub const RELAY_PROTOCOL: u32 = 1;

/// The WebSocket sub-protocol an agent offers when it opens the relay socket.
pub const RELAY_SUBPROTOCOL: &str = "reseek-relay-v1";

/// One message on the relay socket, sent as one JSON text frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RelayEnvelope {
  /// the protocol version
  pub v: u32,
  /// ties a reply to its request: set by the controller on a request and echoed by the
  /// agent's reply; null for messages that are neither
  #[serde(default)]
  pub id: Option<Uuid>,
  /// the message type and its data
  #[serde(flatten)]
  pub message: RelayMessage,
}

/// A relay socket message, written as its `type` and its `data` object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum RelayMessage {
  /// controller to agent, as soon as the socket opens: the token the agent's login must answer
  #[serde(rename_all = "camelCase")]
  Challenge {
    /// a random base62 string, valid on this socket only
    token: String,
    /// how long the token is valid, in milliseconds
    expires_in: u64,
    /// the controller's protocol version
    protocol: u32,
  },
  /// agent to controller: the answer to the challenge
  #[serde(rename_all = "camelCase")]
  Login {
    /// the agent's instance name
    agent: String,
    /// the credential for the challenge token, lower-case hex
    credential: String,
    /// the agent's version
    version: String,
    /// optional features the agent supports, empty today
    capabilities: Vec<String>,
  },
  /// controller to agent: the login was accepted and the agent should send its shares
  #[serde(rename_all = "camelCase")]
  LoginOk {
    /// the instance name the agent was registered under
    agent: String,
    /// the controller's version
    server_version: String,
    /// the protocol version both sides speak
    protocol: u32,
    /// how long the controller waits for the agent's answers
    timeouts: RelayTimeouts,
  },
  /// controller to agent: the login was refused; the controller closes the socket after it
  #[serde(rename_all = "camelCase")]
  LoginFailed {
    /// why
    reason: LoginFailure,
  },
  /// agent to controller: asks to upload its share database
  #[serde(rename_all = "camelCase")]
  SharesBegin {
    /// the version of the agent's share database schema, which must equal the controller's
    schema_version: u32,
    /// the agent's configured shares
    shares: Vec<Share>,
    /// files over the agent's shares
    file_count: u32,
    /// directories over the agent's shares
    directory_count: u32,
    /// the size of the database the agent will upload, in bytes
    database_bytes: u64,
  },
  /// controller to agent: the token to upload the share database with
  #[serde(rename_all = "camelCase")]
  SharesToken {
    /// the single-use upload token
    token: Uuid,
    /// how long the token is valid, in milliseconds
    expires_in: u64,
  },
  /// controller to agent: the uploaded share database is loaded and the agent serves files
  #[serde(rename_all = "camelCase")]
  SharesReady {
    /// files the controller loaded
    file_count: u32,
    /// directories the controller loaded
    directory_count: u32,
  },
  /// controller to agent: asks whether a shared file exists and how long it is
  #[serde(rename_all = "camelCase")]
  FileInfoRequest {
    /// the remote filename
    filename: String,
  },
  /// agent to controller: the answer to a file info request
  #[serde(rename_all = "camelCase")]
  FileInfo {
    /// the file exists on the agent's disk
    exists: bool,
    /// its size on disk in bytes, 0 when it does not exist
    length: u64,
  },
  /// controller to agent: asks the agent to upload a file's bytes for a transfer to a peer
  #[serde(rename_all = "camelCase")]
  FileRequest {
    /// the remote filename
    filename: String,
    /// the byte to start at, as the peer asked
    start_offset: u64,
    /// the upload this is for
    transfer_id: Uuid,
    /// the token to upload the bytes with, the same as the envelope's id
    token: Uuid,
  },
  /// agent to controller: the file is open and its upload has started
  FileAck {},
  /// agent to controller: a file request cannot be served
  #[serde(rename_all = "camelCase")]
  FileFailed {
    /// why
    reason: FileFailure,
    /// the HTTP status the controller answered the upload with, null unless the upload was
    /// rejected
    #[serde(default)]
    status: Option<u16>,
    /// more about the failure, null when the reason says it all
    detail: Option<String>,
  },
  /// controller to agent: a download finished and the agent may fetch a copy
  #[serde(rename_all = "camelCase")]
  DownloadAvailable {
    /// the file's path relative to the controller's downloads directory, with the controller's
    /// separators
    filename: String,
    /// the token to fetch the file with
    token: Uuid,
    /// the file's size in bytes
    size: u64,
    /// how long the token is valid, in milliseconds
    expires_in: u64,
  },
  /// agent to controller: what became of a download notification
  #[serde(rename_all = "camelCase")]
  DownloadDone {
    /// the outcome
    outcome: DownloadOutcome,
    /// more about the outcome, null when there is nothing to add
    detail: Option<String>,
  },
  /// agent to controller: the agent found a file missing and asked for a scan of its own shares
  #[serde(rename_all = "camelCase")]
  RescanRequested {
    /// why
    reason: String,
  },
  /// agent to controller: the agent's own shares are being scanned, so it sends them once the
  /// scan ends; the controller waits longer for them
  #[serde(rename_all = "camelCase")]
  SyncDeferred {
    /// why, such as `scanning`
    reason: String,
  },
  /// either side: a message could not be handled
  #[serde(rename_all = "camelCase")]
  Error {
    /// what went wrong
    code: RelayErrorCode,
    /// more about it, null when the code says it all
    detail: Option<String>,
    /// the id of the message it is about, null when it had none
    #[serde(default)]
    id: Option<Uuid>,
  },
  /// either side: keeps the socket alive through proxies that drop WebSocket control frames
  Ping {},
  /// either side: the answer to a ping
  Pong {},
}

/// How long a controller waits for an agent, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelayTimeouts {
  /// for the answer to a file info request
  pub file_info: u64,
  /// for the acknowledgment of a file request
  pub file_ack: u64,
  /// after the acknowledgment, for the file's bytes to start arriving
  pub file_stream: u64,
}

/// Why a controller refused an agent's login.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoginFailure {
  /// no configured agent has the instance name
  UnknownAgent,
  /// the agent's address is outside its configured networks
  AddressNotAllowed,
  /// the challenge expired before the login arrived
  ChallengeExpired,
  /// the credential does not match
  BadCredential,
  /// the agent's version is outside the controller's release line
  IncompatibleVersion,
}

/// Why an agent cannot serve a file request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileFailure {
  /// the file is not in the agent's shares or not on its disk
  NotFound,
  /// the file could not be opened or read
  Unreadable,
  /// the controller refused the upload
  Rejected,
  /// the agent is stopping
  Cancelled,
  /// any other failure
  Error,
}

/// What an agent did with a download notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DownloadOutcome {
  /// the copy was written
  Stored,
  /// not fetched: downloads are off, the filter excludes it, or a file of the same length is
  /// already there
  Skipped,
  /// every attempt failed
  Failed,
}

/// What a relay socket error is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelayErrorCode {
  /// the message's protocol version or type is unknown; it was ignored
  Unsupported,
  /// the agent's share database schema differs from the controller's
  SchemaMismatch,
  /// a share upload from the agent is already being processed
  SyncInProgress,
  /// the share database is larger than the controller accepts
  TooLarge,
  /// an answer arrived for a request nobody is waiting on
  NoPendingRequest,
}

/// The multipart form of `POST /relay/controller/shares/{token}`, with which an agent uploads
/// its share database. It describes the form in the API document; the controller reads the
/// parts as a stream.
#[derive(Clone, Debug, PartialEq, Eq, ToSchema)]
pub struct RelayShareUpload {
  /// a JSON part: the agent's configured shares
  pub shares: Vec<Share>,
  /// a file part: the agent's share database, an SQLite file
  #[schema(value_type = String, format = Binary)]
  pub database: Vec<u8>,
}

/// The multipart form of `POST /relay/controller/files/{token}`, with which an agent streams a
/// file's bytes for an upload to a peer. It describes the form in the API document; the
/// controller reads the part as a stream.
#[derive(Clone, Debug, PartialEq, Eq, ToSchema)]
pub struct RelayFileUpload {
  /// a file part named after the remote filename: the file's bytes from the requested offset
  #[schema(value_type = String, format = Binary)]
  pub file: Vec<u8>,
}

/// How a relayed upload ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelayUploadOutcome {
  /// the peer received the whole file
  Succeeded,
  /// the upload was canceled here or by the peer
  Cancelled,
  /// the transfer to the peer failed
  Failed,
}

/// The answer to `POST /relay/controller/files/{token}`, sent once the transfer to the peer
/// ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelayFileUploadResult {
  /// how the upload ended
  pub outcome: RelayUploadOutcome,
  /// how many bytes reached the peer
  pub bytes: u64,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn envelope_puts_type_and_data_beside_version_and_id() {
    let id = Uuid::from_u128(7);
    let envelope = RelayEnvelope {
      v: RELAY_PROTOCOL,
      id: Some(id),
      message: RelayMessage::FileInfo {
        exists: true,
        length: 42,
      },
    };
    let json = serde_json::to_value(&envelope).unwrap();
    assert_eq!(
      json,
      serde_json::json!({
        "v": 1,
        "id": id,
        "type": "file_info",
        "data": { "exists": true, "length": 42 }
      })
    );
    assert_eq!(
      serde_json::from_value::<RelayEnvelope>(json).unwrap(),
      envelope
    );
  }

  #[test]
  fn envelope_without_id_and_empty_data_reads() {
    let read: RelayEnvelope =
      serde_json::from_str(r#"{"v":1,"type":"file_ack","data":{}}"#).unwrap();
    assert_eq!(read.id, None);
    assert_eq!(read.message, RelayMessage::FileAck {});
    assert_eq!(
      serde_json::to_string(&RelayEnvelope {
        v: 1,
        id: None,
        message: RelayMessage::Ping {},
      })
      .unwrap(),
      r#"{"v":1,"id":null,"type":"ping","data":{}}"#
    );
  }
}
