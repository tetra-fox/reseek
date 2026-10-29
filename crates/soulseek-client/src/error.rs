//! The one error type every client operation returns. Callers match on the variant (and on the
//! kind of the grouped variants); the message says what failed and where, and the source carries
//! the underlying error when there is one.

use std::error::Error as StdError;
use std::sync::Arc;

use soulseek_proto::DecodeError;

/// The underlying error of a wrapping variant. Shared so errors can be cloned into events and
/// handed to several waiters.
pub type ErrorSource = Arc<dyn StdError + Send + Sync>;

/// Everything that can go wrong in the client
#[derive(Clone, Debug, thiserror::Error)]
pub enum ClientError {
  /// the caller passed a value the operation does not accept; nothing was sent
  #[error("{0}")]
  InvalidArgument(String),
  /// the client or the connection is in a state that does not allow the operation
  #[error("{0}")]
  InvalidOperation(String),
  /// a wait, a connect or an inactivity deadline expired
  #[error("{0}")]
  Timeout(String),
  /// the caller canceled, or the client was disconnected while the operation ran
  #[error("the operation was canceled")]
  Canceled,
  /// the server address could not be resolved
  #[error("{0}")]
  Address(String),
  /// the listener could not bind its address
  #[error("{message}")]
  Listen {
    message: String,
    #[source]
    source: Option<ErrorSource>,
  },
  /// connecting to the server failed for a reason other than a rejected or unanswered login
  #[error("{message}")]
  Connect {
    message: String,
    #[source]
    source: Option<ErrorSource>,
  },
  /// the server refused the login; the message carries its reason verbatim
  #[error("{0}")]
  LoginRejected(String),
  /// the login was written and the connection stayed up, but the server never answered it,
  /// which is how the server treats a banned account
  #[error("{0}")]
  LoginNoResponse(String),
  /// another session logged in with our credentials
  #[error("Kicked from server")]
  KickedFromServer,
  /// a socket-level failure on one connection
  #[error("{message}")]
  Connection {
    kind: ConnectionErrorKind,
    message: String,
    #[source]
    source: Option<ErrorSource>,
  },
  /// a frame that could not be read, decoded or encoded
  #[error("{message}")]
  Message {
    kind: MessageErrorKind,
    message: String,
    #[source]
    source: Option<ErrorSource>,
  },
  /// the server reports the user as offline
  #[error("{0}")]
  UserOffline(String),
  /// the server reports that the user does not exist
  #[error("{0}")]
  UserNotFound(String),
  /// asking the server for a user's address failed
  #[error("{message}")]
  UserEndPoint {
    message: String,
    #[source]
    source: Option<ErrorSource>,
  },
  /// a chat room request failed
  #[error("{message}")]
  Room {
    kind: RoomErrorKind,
    message: String,
  },
  /// the server did not answer a room join or leave, which it does for a room we are already
  /// in or not in
  #[error("{0}")]
  NoResponse(String),
  /// a resolver refused to queue a download; the reason is sent to the peer as written
  #[error("{0}")]
  DownloadEnqueue(String),
  /// the token is already used by another search, or by another transfer
  #[error("token {0} is already in use")]
  DuplicateToken(u32),
  /// a transfer of the same file with the same user in the same direction is already active
  #[error("a transfer of {filename} with {username} is already active")]
  DuplicateTransfer { username: String, filename: String },
  /// no download of the file from the user is active
  #[error("no active download of {filename} from {username}")]
  TransferNotFound { username: String, filename: String },
  /// the local file to upload does not exist
  #[error("local file not found: {0}")]
  NotFound(String),
  /// the peer refused or canceled a transfer, with its reason, which is kept as it was sent
  #[error("transfer rejected: {0}")]
  TransferRejected(String),
  /// the uploader reported that the upload of a file to us failed
  #[error("{0}")]
  TransferReportedFailed(String),
  /// the size the peer reports for a file differs from the size we expect, and nothing settled
  /// which one is right
  #[error("transfer aborted: size mismatch (local {local}, remote {remote})")]
  SizeMismatch {
    local: u64,
    remote: u64,
    #[source]
    source: Option<ErrorSource>,
  },
  /// a transfer could not go on: no upload slot, a bad start offset, a source that ended early
  /// or changed size, a stream that cannot seek
  #[error("transfer error: {message}")]
  Transfer {
    message: String,
    #[source]
    source: Option<ErrorSource>,
  },
  /// reading or writing a local file or stream failed
  #[error("I/O error: {message}")]
  Io {
    message: String,
    #[source]
    source: Option<ErrorSource>,
  },
  /// an operation failed for another reason; the message names the operation
  #[error("{message}")]
  Client {
    message: String,
    #[source]
    source: Option<ErrorSource>,
  },
  /// the client has been disposed and accepts no more work
  #[error("the client has been disposed")]
  Disposed,
}

/// Which socket-level failure a [`ClientError::Connection`] is
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionErrorKind {
  /// connecting failed, or the connection closed or was refused for another reason
  Other,
  /// a read failed or the remote closed the connection
  Read,
  /// a write failed
  Write,
  /// the connection's write queue was full, so the write was dropped and the connection closed
  WriteDropped,
  /// the SOCKS5 proxy refused or failed the connection
  Proxy,
}

/// Which framing failure a [`ClientError::Message`] is
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageErrorKind {
  /// a message could not be encoded or handled
  Other,
  /// a frame could not be read or decoded
  Read,
  /// a compressed payload could not be inflated
  Compression,
}

/// Which room failure a [`ClientError::Room`] is
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomErrorKind {
  /// any other room failure
  Other,
  /// joining would create a room whose name a private room we are not in already holds
  JoinForbidden,
}

impl ClientError {
  /// a socket-level error without an underlying cause
  pub fn connection(kind: ConnectionErrorKind, message: impl Into<String>) -> Self {
    Self::Connection {
      kind,
      message: message.into(),
      source: None,
    }
  }

  /// a socket-level error caused by `source`
  pub fn connection_from(
    kind: ConnectionErrorKind,
    message: impl Into<String>,
    source: impl StdError + Send + Sync + 'static,
  ) -> Self {
    Self::Connection {
      kind,
      message: message.into(),
      source: Some(Arc::new(source)),
    }
  }

  /// a frame that failed to decode
  pub fn message_read(message: impl Into<String>, source: DecodeError) -> Self {
    Self::Message {
      kind: MessageErrorKind::Read,
      message: message.into(),
      source: Some(Arc::new(source)),
    }
  }

  /// the generic wrapper an operation returns, naming the operation in `message`
  pub fn client(message: impl Into<String>, source: Option<ClientError>) -> Self {
    Self::Client {
      message: message.into(),
      source: source.map(|source| Arc::new(source) as ErrorSource),
    }
  }

  /// a transfer that could not go on, without an underlying cause
  pub fn transfer(message: impl Into<String>) -> Self {
    Self::Transfer {
      message: message.into(),
      source: None,
    }
  }

  /// a local I/O failure, described by `message`
  pub fn io(message: impl Into<String>, source: std::io::Error) -> Self {
    Self::Io {
      message: message.into(),
      source: Some(Arc::new(source)),
    }
  }

  /// the connection error of a given kind, or `None` for any other variant
  pub fn connection_kind(&self) -> Option<ConnectionErrorKind> {
    match self {
      Self::Connection { kind, .. } => Some(*kind),
      _ => None,
    }
  }
}
