//! What an accepted connection is for, read from its first message: a peer introducing itself
//! for messages, a file transfer or the distributed tree, or a peer answering a connection
//! request we had the server relay. Anything else is closed.

use std::fmt::Write as _;

use soulseek_proto::init::{PeerInit, PierceFirewall};
use soulseek_proto::{DecodeError, FieldError, InitMessage, PeerConnectionType};

use crate::conn::Conn;
use crate::error::{ClientError, ConnectionErrorKind};

/// The first message is at most a username and a connection type; anything longer is not one.
const MAX_INIT_BYTES: u32 = 1024;

/// how many leading bytes of an unrecognised first message an error shows
const SHOWN_BYTES: usize = 16;

/// An accepted connection and what its first message says it is for
#[derive(Debug)]
pub(crate) enum InboundConnection {
  /// a peer opening a message connection to us
  Peer { username: String, conn: Conn },
  /// a peer opening a file transfer connection; the token it sent is its own, and the token we
  /// assigned the transfer follows on the connection
  Transfer {
    username: String,
    token: u32,
    conn: Conn,
  },
  /// a peer joining the distributed search tree below us
  Distributed { username: String, conn: Conn },
  /// a peer connecting because the server relayed one of our connection requests; `token` is
  /// the token of that request
  PierceFirewall { token: u32, conn: Conn },
}

/// Reads the first message of an accepted connection and says what it is for, or closes it when
/// it is for nothing we know. The connection's own inactivity timeout bounds how long a silent
/// peer can hold it.
pub(crate) async fn classify(mut conn: Conn) -> Option<InboundConnection> {
  let remote = conn.handle().remote();
  let init = match read_init(&mut conn).await {
    Ok(init) => init,
    // the connection logs why it closed, in its own span with the remote address
    Err(error) => {
      conn.handle().disconnect(error.to_string(), Some(error));
      return None;
    }
  };
  Some(match init {
    InitMessage::PeerInit(PeerInit {
      username,
      connection_type,
      token,
    }) => {
      tracing::debug!(%remote, username, ?connection_type, token, "inbound connection identified");
      match connection_type {
        PeerConnectionType::Peer => InboundConnection::Peer { username, conn },
        PeerConnectionType::Transfer => InboundConnection::Transfer {
          username,
          token,
          conn,
        },
        PeerConnectionType::Distributed => InboundConnection::Distributed { username, conn },
      }
    }
    InitMessage::PierceFirewall(PierceFirewall { token }) => {
      tracing::debug!(%remote, token, "inbound connection answers a relayed request");
      InboundConnection::PierceFirewall { token, conn }
    }
  })
}

async fn read_init(conn: &mut Conn) -> Result<InitMessage, ClientError> {
  let mut frame = conn.read_exact(4).await?;
  let len = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
  if len > MAX_INIT_BYTES {
    return Err(unrecognized(&frame, len));
  }
  frame.extend(conn.read_exact(len as usize).await?);
  InitMessage::decode(&frame).map_err(|error| match error {
    DecodeError::Field {
      error: FieldError::UnknownConnectionType,
      ..
    } => ClientError::connection(ConnectionErrorKind::Other, "unknown connection type"),
    _ => unrecognized(&frame, len),
  })
}

fn unrecognized(frame: &[u8], len: u32) -> ClientError {
  let mut hex = String::new();
  for byte in frame.iter().take(SHOWN_BYTES) {
    // writing to a String cannot fail
    let _ = write!(hex, "{byte:02x}");
  }
  if frame.len() > SHOWN_BYTES {
    hex.push_str("...");
  }
  ClientError::connection(
    ConnectionErrorKind::Other,
    format!(
      "Unrecognized initialization message: {hex} ({} bytes)",
      u64::from(len) + 4
    ),
  )
}

#[cfg(test)]
mod tests {
  use tokio::io::AsyncWriteExt;
  use tokio::net::{TcpListener, TcpStream};

  use super::*;
  use crate::options::ConnectionOptions;

  /// an accepted connection whose remote end has sent `bytes`, and that remote end
  async fn accepted_with(bytes: &[u8]) -> (Conn, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut remote = TcpStream::connect(listener.local_addr().unwrap())
      .await
      .unwrap();
    let (stream, address) = listener.accept().await.unwrap();
    remote.write_all(bytes).await.unwrap();
    (
      Conn::accepted(stream, address, ConnectionOptions::default()),
      remote,
    )
  }

  async fn classified(bytes: &[u8]) -> InboundConnection {
    let (conn, _remote) = accepted_with(bytes).await;
    classify(conn).await.expect("the connection is classified")
  }

  /// the reason a connection that sent `bytes` was closed with
  async fn rejection(bytes: &[u8]) -> String {
    let (conn, _remote) = accepted_with(bytes).await;
    let handle = conn.handle().clone();
    assert!(classify(conn).await.is_none(), "nothing is classified");
    handle.disconnected().await.message
  }

  fn peer_init(username: &str, connection_type: &str, token: u32) -> Vec<u8> {
    let mut payload = vec![1];
    for text in [username, connection_type] {
      payload.extend((text.len() as u32).to_le_bytes());
      payload.extend(text.as_bytes());
    }
    payload.extend(token.to_le_bytes());
    let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
    frame.extend(payload);
    frame
  }

  #[tokio::test]
  async fn peer_init_types_are_told_apart() {
    assert!(matches!(
      classified(&peer_init("alice", "P", 0)).await,
      InboundConnection::Peer { username, .. } if username == "alice"
    ));
    assert!(matches!(
      classified(&peer_init("alice", "F", 77)).await,
      InboundConnection::Transfer { username, token: 77, .. } if username == "alice"
    ));
    assert!(matches!(
      classified(&peer_init("alice", "D", 0)).await,
      InboundConnection::Distributed { username, .. } if username == "alice"
    ));
  }

  #[tokio::test]
  async fn pierce_firewall_carries_its_token() {
    assert!(matches!(
      classified(&[5, 0, 0, 0, 0, 42, 0, 0, 0]).await,
      InboundConnection::PierceFirewall { token: 42, .. }
    ));
  }

  #[tokio::test]
  async fn an_unknown_connection_type_is_closed_at_once() {
    assert_eq!(
      rejection(&peer_init("alice", "X", 0)).await,
      "unknown connection type"
    );
  }

  #[tokio::test]
  async fn a_first_message_over_1_kib_is_rejected_from_its_length() {
    assert_eq!(
      rejection(&1025u32.to_le_bytes()).await,
      "Unrecognized initialization message: 01040000 (1029 bytes)"
    );
  }

  #[tokio::test]
  async fn an_unknown_first_message_is_rejected() {
    assert_eq!(
      rejection(&[2, 0, 0, 0, 9, 9]).await,
      "Unrecognized initialization message: 020000000909 (6 bytes)"
    );
  }
}
