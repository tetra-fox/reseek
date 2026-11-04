//! The relay socket's frames as both sides read and write them: one JSON envelope per text
//! frame, the close codes, and how long each side waits.

use std::time::Duration;

use api_types::relay::{RELAY_PROTOCOL, RelayEnvelope, RelayErrorCode, RelayMessage};
use uuid::Uuid;

/// How often each side pings the other.
pub const PING_EVERY: Duration = Duration::from_secs(15);

/// How long either side waits for any frame before it closes the socket.
pub const SILENCE: Duration = Duration::from_secs(30);

/// How long a login challenge is valid, and how long an agent has from the socket's opening to
/// its login.
pub const LOGIN_WITHIN: Duration = Duration::from_secs(10);

/// How long a controller waits after a login, or after the agent last said its scan runs, for
/// the agent to start sending its shares.
pub const SYNC_WITHIN: Duration = Duration::from_secs(120);

/// How often an agent whose shares are being scanned says so while it has not sent them.
pub const DEFER_EVERY: Duration = Duration::from_secs(60);

/// How long a share upload token is valid.
pub const SHARE_TOKEN_LIFETIME: Duration = Duration::from_secs(300);

/// How long a finished download's token is valid, for every attempt to fetch it.
pub const DOWNLOAD_TOKEN_LIFETIME: Duration = Duration::from_secs(600);

/// The largest frame either side reads.
pub const MAX_FRAME: usize = 1 << 20;

/// The close codes of the relay socket.
pub mod close {
  /// closed on purpose
  pub const NORMAL: u16 = 1000;
  /// a frame was larger than either side reads
  pub const TOO_BIG: u16 = 1009;
  /// another socket logged in as the same agent
  pub const SUPERSEDED: u16 = 4001;
  /// the controller's operator disconnected the agent, or its configuration changed
  pub const KICKED: u16 = 4002;
  /// the login was refused
  pub const UNAUTHORIZED: u16 = 4003;
  /// the login, the share upload or any frame did not arrive in time
  pub const TIMEOUT: u16 = 4008;
  /// the controller is stopping
  pub const SHUTTING_DOWN: u16 = 4010;
}

/// A frame that is not a message this side reads: an unknown version or type, or data that
/// does not fit its type. It is answered with an `unsupported` error and otherwise ignored.
#[derive(Debug, PartialEq)]
pub struct Unsupported {
  /// the frame's id, when it had one
  pub id: Option<Uuid>,
  /// what was wrong with it
  pub detail: String,
}

impl Unsupported {
  /// The error message that answers the frame.
  pub fn answer(self) -> String {
    encode(
      None,
      RelayMessage::Error {
        code: RelayErrorCode::Unsupported,
        detail: Some(self.detail),
        id: self.id,
      },
    )
  }
}

/// Reads a text frame.
pub fn decode(text: &str) -> Result<RelayEnvelope, Unsupported> {
  let value: serde_json::Value = serde_json::from_str(text).map_err(|e| Unsupported {
    id: None,
    detail: format!("not JSON: {e}"),
  })?;
  let id = value
    .get("id")
    .and_then(|id| id.as_str())
    .and_then(|id| Uuid::parse_str(id).ok());
  let version = value.get("v").and_then(serde_json::Value::as_u64);
  if version != Some(u64::from(RELAY_PROTOCOL)) {
    return Err(Unsupported {
      id,
      detail: format!("protocol version {version:?} is not {RELAY_PROTOCOL}"),
    });
  }
  serde_json::from_value(value).map_err(|e| Unsupported {
    id,
    detail: e.to_string(),
  })
}

/// Writes `message` as a text frame, with `id` when it is a request or its reply.
pub fn encode(id: Option<Uuid>, message: RelayMessage) -> String {
  serde_json::to_string(&RelayEnvelope {
    v: RELAY_PROTOCOL,
    id,
    message,
  })
  .expect("a relay message always serializes")
}

/// `duration` in whole milliseconds, as the socket's messages give durations.
pub fn millis(duration: Duration) -> u64 {
  u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// The name of `message`'s type, for logs.
pub fn kind(message: &RelayMessage) -> &'static str {
  match message {
    RelayMessage::Challenge { .. } => "challenge",
    RelayMessage::Login { .. } => "login",
    RelayMessage::LoginOk { .. } => "login_ok",
    RelayMessage::LoginFailed { .. } => "login_failed",
    RelayMessage::SharesBegin { .. } => "shares_begin",
    RelayMessage::SharesToken { .. } => "shares_token",
    RelayMessage::SharesReady { .. } => "shares_ready",
    RelayMessage::FileInfoRequest { .. } => "file_info_request",
    RelayMessage::FileInfo { .. } => "file_info",
    RelayMessage::FileRequest { .. } => "file_request",
    RelayMessage::FileAck {} => "file_ack",
    RelayMessage::FileFailed { .. } => "file_failed",
    RelayMessage::DownloadAvailable { .. } => "download_available",
    RelayMessage::DownloadDone { .. } => "download_done",
    RelayMessage::RescanRequested { .. } => "rescan_requested",
    RelayMessage::SyncDeferred { .. } => "sync_deferred",
    RelayMessage::Error { .. } => "error",
    RelayMessage::Ping {} => "ping",
    RelayMessage::Pong {} => "pong",
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_unknown_version_or_type_is_unsupported_with_its_id() {
    let id = Uuid::from_u128(9);
    let newer = decode(&format!(
      r#"{{"v":2,"id":"{id}","type":"ping","data":{{}}}}"#
    ));
    assert_eq!(newer.unwrap_err().id, Some(id));
    let unknown = decode(r#"{"v":1,"type":"teleport","data":{}}"#).unwrap_err();
    assert_eq!(unknown.id, None);
    assert!(decode("not json").is_err());
    let answer: serde_json::Value = serde_json::from_str(&unknown.answer()).unwrap();
    assert_eq!(answer["type"], "error");
    assert_eq!(answer["data"]["code"], "unsupported");
    assert_eq!(
      decode(&encode(Some(id), RelayMessage::Pong {})).unwrap().id,
      Some(id)
    );
  }
}
