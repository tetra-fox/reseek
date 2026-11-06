//! What an accepted connection is for, read from its first message: a peer introducing itself
//! for messages, a file transfer or the distributed tree, or a peer answering a connection
//! request we had the server relay. Anything else is closed.

use std::fmt::Write as _;

use soulseek_proto::init::{PeerInit, PierceFirewall};
use soulseek_proto::{
  DecodeError, FieldError, InitMessage, LIMIT_INIT_MESSAGE_BYTES, PeerConnectionType,
};
use tokio::time::timeout;

use crate::conn::Conn;
use crate::error::{ClientError, ConnectionErrorKind};

/// how many leading bytes of an unrecognized first message an error shows
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
/// it is for nothing we know. The whole message must arrive within the connection's connect
/// timeout, so a peer sending a byte now and then cannot hold the socket past it.
pub(crate) async fn classify(mut conn: Conn) -> Option<InboundConnection> {
  let remote = conn.handle().remote();
  let deadline = conn.handle().options().connect_timeout;
  let init = timeout(deadline, read_init(&mut conn))
    .await
    .unwrap_or_else(|_| {
      Err(ClientError::Timeout(format!(
        "No initialization message within {} milliseconds",
        deadline.as_millis()
      )))
    });
  let init = match init {
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
  if len > LIMIT_INIT_MESSAGE_BYTES {
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
  use std::time::Duration;

  use tokio::io::AsyncWriteExt;
  use tokio::net::{TcpListener, TcpStream};

  use super::*;
  use crate::options::ConnectionOptions;

  /// an accepted connection with `options` whose remote end has sent `bytes`, and that remote
  /// end
  async fn accepted_with(options: ConnectionOptions, bytes: &[u8]) -> (Conn, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut remote = TcpStream::connect(listener.local_addr().unwrap())
      .await
      .unwrap();
    let (stream, address) = listener.accept().await.unwrap();
    remote.write_all(bytes).await.unwrap();
    (Conn::accepted(stream, address, options), remote)
  }

  /// the reason a connection that sent `bytes` was closed with
  async fn rejection(bytes: &[u8]) -> String {
    let (conn, _remote) = accepted_with(ConnectionOptions::default(), bytes).await;
    let handle = conn.handle().clone();
    assert!(classify(conn).await.is_none(), "nothing is classified");
    handle.disconnected().await.message
  }

  #[tokio::test]
  async fn a_first_message_over_the_limit_is_rejected_from_its_length() {
    assert_eq!(
      rejection(&(LIMIT_INIT_MESSAGE_BYTES + 1).to_le_bytes()).await,
      "Unrecognized initialization message: 01100000 (4101 bytes)"
    );
  }

  #[tokio::test]
  async fn a_first_message_still_arriving_at_the_deadline_is_rejected() {
    let options = ConnectionOptions {
      connect_timeout: Duration::from_millis(200),
      inactivity_timeout: None,
      ..ConnectionOptions::default()
    };
    // the length promises a frame that never comes
    let (conn, _remote) = accepted_with(options, &[9, 0]).await;
    let handle = conn.handle().clone();
    assert!(classify(conn).await.is_none(), "nothing is classified");
    assert_eq!(
      handle.disconnected().await.message,
      "No initialization message within 200 milliseconds"
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
