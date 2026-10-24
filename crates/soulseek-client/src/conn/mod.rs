//! One TCP connection to one remote: connecting (directly or through a SOCKS5 proxy, under one
//! deadline), exact-length reads and chunked writes that count as activity, the inactivity
//! timeout, and a disconnect that happens exactly once with a reason. A [`Conn`] owns the socket;
//! a [`ConnHandle`] is the cheap shared view everything else holds. Message connections
//! ([`MessageConn`]) run a framing read loop and a writer task on top.

mod handle;
mod message;
mod socket;
mod socks5;

pub use handle::{ConnHandle, ConnKey, ConnKind, ConnState, Disconnect};
pub use message::{MessageConn, ReadEvent};
pub(crate) use socket::{configure, resolve};

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use socket2::SockRef;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::error::{ClientError, ConnectionErrorKind};
use crate::options::ConnectionOptions;

/// A connection that owns its socket: reads and writes go straight to it. Accepted connections
/// start as one, and transfer connections stay one; message connections are made from one with
/// [`Conn::into_message`]. Dropping it closes the socket.
pub struct Conn {
  handle: ConnHandle,
  stream: Option<TcpStream>,
}

impl fmt::Debug for Conn {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.handle.fmt(f)
  }
}

impl Conn {
  /// Connects to `remote`, directly or through the options' proxy. The TCP connect and the
  /// proxy handshake share one `connect_timeout` deadline; `cancel` aborts it.
  pub async fn connect(
    remote: SocketAddr,
    username: Option<String>,
    kind: ConnKind,
    options: ConnectionOptions,
    cancel: &CancellationToken,
  ) -> Result<Self, ClientError> {
    let handle = ConnHandle::new(remote, username, kind, options, ConnState::Pending);
    handle.connecting();
    let span = handle.span().clone();
    let connect_timeout = handle.options().connect_timeout;
    let attempt = timeout(connect_timeout, socket::connect(remote, handle.options()));
    let result = tokio::select! {
      biased;
      () = cancel.cancelled() => Err(ClientError::Cancelled),
      attempt = attempt.instrument(span.clone()) => attempt.unwrap_or_else(|_| {
        Err(ClientError::Timeout(format!(
          "Operation timed out after {} milliseconds",
          connect_timeout.as_millis()
        )))
      }),
    };
    match result {
      Ok(stream) => {
        span.in_scope(|| tracing::debug!("connected"));
        handle.connected();
        Ok(Self {
          handle,
          stream: Some(stream),
        })
      }
      Err(error) => {
        handle.disconnect(format!("Connection Error: {error}"), Some(error.clone()));
        Err(match error {
          ClientError::Timeout(_) | ClientError::Cancelled => error,
          error => {
            let message = format!("Failed to connect to {remote}: {error}");
            ClientError::Connection {
              kind: ConnectionErrorKind::Other,
              message,
              source: Some(Arc::new(error)),
            }
          }
        })
      }
    }
  }

  /// Wraps a socket the listener accepted. It has no kind and no username until its first
  /// message is read.
  pub(crate) fn accepted(
    stream: TcpStream,
    remote: SocketAddr,
    options: ConnectionOptions,
  ) -> Self {
    configure(SockRef::from(&stream), &options);
    let handle = ConnHandle::new(remote, None, ConnKind::empty(), options, ConnState::Pending);
    handle.connected();
    Self {
      handle,
      stream: Some(stream),
    }
  }

  /// Wraps a connected socket taken from another connection with [`Conn::handoff`]. The new
  /// connection is connected from the start and gets its own id and options.
  pub fn from_handoff(
    stream: TcpStream,
    username: Option<String>,
    kind: ConnKind,
    options: ConnectionOptions,
  ) -> Result<Self, ClientError> {
    let remote = stream.peer_addr().map_err(|error| {
      ClientError::connection_from(
        ConnectionErrorKind::Other,
        format!("The handed off connection is no longer connected: {error}"),
        error,
      )
    })?;
    configure(SockRef::from(&stream), &options);
    let handle = ConnHandle::new(remote, username, kind, options, ConnState::Pending);
    handle.connected();
    Ok(Self {
      handle,
      stream: Some(stream),
    })
  }

  /// the shared view of this connection
  pub fn handle(&self) -> &ConnHandle {
    &self.handle
  }

  /// Takes the socket out, still open, so another connection object can be made from it. This
  /// connection is left disconnected.
  pub fn handoff(mut self) -> Result<TcpStream, ClientError> {
    let stream = self.take_stream()?;
    self.handle.disconnect("handed off", None);
    Ok(stream)
  }

  /// reads exactly `len` bytes
  pub async fn read_exact(&mut self, len: usize) -> Result<Vec<u8>, ClientError> {
    let mut buf = vec![0; len];
    let handle = self.handle.clone();
    let stream = self.stream_mut()?;
    read_exact(&handle, stream, &mut buf).await?;
    Ok(buf)
  }

  /// writes all of `bytes`
  pub async fn write_all(&mut self, bytes: &[u8]) -> Result<(), ClientError> {
    let handle = self.handle.clone();
    let stream = self.stream_mut()?;
    write_all(&handle, stream, bytes).await
  }

  /// Splits the connection into a reading and a writing half that can be used from different
  /// tasks, each still counting activity and closing the connection on failure.
  pub fn into_split(mut self) -> Result<(ConnReadHalf, ConnWriteHalf), ClientError> {
    let (read, write) = self.take_stream()?.into_split();
    Ok((
      ConnReadHalf {
        handle: self.handle.clone(),
        half: read,
      },
      ConnWriteHalf {
        handle: self.handle.clone(),
        half: write,
      },
    ))
  }

  fn stream_mut(&mut self) -> Result<&mut TcpStream, ClientError> {
    if !self.handle.is_connected() {
      return Err(self.handle.not_connected());
    }
    self
      .stream
      .as_mut()
      .ok_or_else(|| self.handle.not_connected())
  }

  fn take_stream(&mut self) -> Result<TcpStream, ClientError> {
    if !self.handle.is_connected() {
      return Err(self.handle.not_connected());
    }
    self
      .stream
      .take()
      .ok_or_else(|| self.handle.not_connected())
  }
}

impl Drop for Conn {
  fn drop(&mut self) {
    if self.stream.is_some() {
      self.handle.disconnect("Connection is being disposed", None);
    }
  }
}

/// The reading half of a split connection
pub struct ConnReadHalf {
  handle: ConnHandle,
  half: OwnedReadHalf,
}

impl ConnReadHalf {
  /// the connection this half belongs to
  pub fn handle(&self) -> &ConnHandle {
    &self.handle
  }

  /// fills `buf` completely
  pub async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), ClientError> {
    read_exact(&self.handle, &mut self.half, buf).await
  }
}

/// The writing half of a split connection
pub struct ConnWriteHalf {
  handle: ConnHandle,
  half: OwnedWriteHalf,
}

impl ConnWriteHalf {
  /// the connection this half belongs to
  pub fn handle(&self) -> &ConnHandle {
    &self.handle
  }

  /// writes all of `bytes`
  pub async fn write_all(&mut self, bytes: &[u8]) -> Result<(), ClientError> {
    write_all(&self.handle, &mut self.half, bytes).await
  }
}

/// Fills `buf` in reads of at most `read_buffer_size`, each counted as activity. A failure
/// closes the connection, which a read the closing interrupted reports as well.
async fn read_exact<R: AsyncRead + Unpin>(
  handle: &ConnHandle,
  reader: &mut R,
  buf: &mut [u8],
) -> Result<(), ClientError> {
  if !handle.is_connected() {
    return Err(handle.not_connected());
  }
  let len = buf.len();
  let chunk = handle.options().read_buffer_size;
  let mut total = 0;
  handle.touch();
  while total < len {
    let end = len.min(total + chunk);
    let read = tokio::select! {
      biased;
      () = handle.cancellation().cancelled() => {
        return Err(read_error(handle, len, handle.closed_message(), None));
      }
      read = reader.read(&mut buf[total..end]) => read,
    };
    match read {
      Ok(0) => {
        return Err(read_failed(
          handle,
          len,
          "Remote connection closed".into(),
          None,
        ));
      }
      Ok(read) => {
        total += read;
        handle.touch();
      }
      Err(error) => return Err(read_failed(handle, len, error.to_string(), Some(error))),
    }
  }
  Ok(())
}

fn read_error(
  handle: &ConnHandle,
  len: usize,
  reason: String,
  source: Option<std::io::Error>,
) -> ClientError {
  let message = format!(
    "Failed to read {len} bytes from {}: {reason}",
    handle.remote()
  );
  match source {
    Some(source) => ClientError::connection_from(ConnectionErrorKind::Read, message, source),
    None => ClientError::connection(ConnectionErrorKind::Read, message),
  }
}

fn read_failed(
  handle: &ConnHandle,
  len: usize,
  reason: String,
  source: Option<std::io::Error>,
) -> ClientError {
  let error = read_error(handle, len, reason.clone(), source);
  handle.disconnect(format!("Read error: {reason}"), Some(error.clone()));
  error
}

/// Writes `bytes` in writes of at most `write_buffer_size`, each counted as activity. A failure
/// closes the connection; a write the closing interrupts reports how far it got.
async fn write_all<W: AsyncWrite + Unpin>(
  handle: &ConnHandle,
  writer: &mut W,
  bytes: &[u8],
) -> Result<(), ClientError> {
  if bytes.is_empty() {
    return Err(ClientError::InvalidArgument(
      "Invalid attempt to send empty data".into(),
    ));
  }
  if !handle.is_connected() {
    return Err(handle.not_connected());
  }
  let chunk = handle.options().write_buffer_size;
  handle.touch();
  let mut total = 0;
  while total < bytes.len() {
    let end = bytes.len().min(total + chunk);
    let written = tokio::select! {
      biased;
      () = handle.cancellation().cancelled() => {
        return Err(ClientError::connection(
          ConnectionErrorKind::Write,
          format!(
            "Write aborted after {total} bytes written; the connection has been or is being disposed/disconnected"
          ),
        ));
      }
      written = writer.write(&bytes[total..end]) => written,
    };
    match written {
      Ok(0) => {
        let error = std::io::Error::from(std::io::ErrorKind::WriteZero);
        return Err(write_failed(handle, bytes.len(), error));
      }
      Ok(written) => {
        total += written;
        handle.touch();
      }
      Err(error) => return Err(write_failed(handle, bytes.len(), error)),
    }
  }
  Ok(())
}

fn write_failed(handle: &ConnHandle, len: usize, source: std::io::Error) -> ClientError {
  let reason = source.to_string();
  let error = ClientError::connection_from(
    ConnectionErrorKind::Write,
    format!(
      "Failed to write {len} bytes to {}: {reason}",
      handle.remote()
    ),
    source,
  );
  handle.disconnect(format!("Write error: {reason}"), Some(error.clone()));
  error
}

#[cfg(test)]
pub(crate) mod tests {
  use std::time::Duration;

  use tokio::net::TcpListener;

  use super::*;

  /// a connection to a loopback listener and the listener's end of it
  pub(crate) async fn pair(options: ConnectionOptions) -> (Conn, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote = listener.local_addr().unwrap();
    let conn = Conn::connect(
      remote,
      Some("peer".into()),
      ConnKind::OUTBOUND | ConnKind::DIRECT,
      options,
      &CancellationToken::new(),
    )
    .await
    .unwrap();
    let (accepted, _) = listener.accept().await.unwrap();
    (conn, accepted)
  }

  #[tokio::test]
  async fn remote_close_is_a_read_error_that_disconnects() {
    let (mut conn, remote) = pair(ConnectionOptions::default()).await;
    drop(remote);
    let error = conn.read_exact(4).await.unwrap_err();
    assert_eq!(error.connection_kind(), Some(ConnectionErrorKind::Read));
    assert!(
      error.to_string().ends_with("Remote connection closed"),
      "{error}"
    );
    let reason = conn.handle().disconnected().await;
    assert_eq!(reason.message, "Read error: Remote connection closed");
  }

  #[tokio::test]
  async fn handoff_keeps_the_socket_open() {
    let (conn, mut remote) = pair(ConnectionOptions::default()).await;
    let old = conn.handle().clone();
    let stream = conn.handoff().unwrap();
    assert!(matches!(old.state(), ConnState::Disconnected(_)));
    let mut new = Conn::from_handoff(
      stream,
      Some("peer".into()),
      ConnKind::INBOUND | ConnKind::DIRECT,
      ConnectionOptions::default(),
    )
    .unwrap();
    assert_ne!(new.handle().id(), old.id());
    remote.write_all(b"ok").await.unwrap();
    assert_eq!(new.read_exact(2).await.unwrap(), b"ok");
  }

  #[tokio::test]
  async fn refused_connect_is_a_connection_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote = listener.local_addr().unwrap();
    drop(listener);
    let error = Conn::connect(
      remote,
      None,
      ConnKind::OUTBOUND | ConnKind::DIRECT,
      ConnectionOptions::default(),
      &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.connection_kind(), Some(ConnectionErrorKind::Other));
    assert!(
      error
        .to_string()
        .starts_with(&format!("Failed to connect to {remote}: ")),
      "{error}"
    );
  }

  #[tokio::test]
  async fn a_stalled_proxy_handshake_counts_against_the_connect_deadline() {
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_address = proxy.local_addr().unwrap();
    // accepts the TCP connection and never answers the handshake
    let stall = tokio::spawn(async move {
      let (socket, _) = proxy.accept().await.unwrap();
      std::future::pending::<()>().await;
      drop(socket);
    });
    let options = ConnectionOptions {
      connect_timeout: Duration::from_millis(200),
      proxy: Some(crate::ProxyOptions {
        address: proxy_address.ip().to_string(),
        port: proxy_address.port(),
        credentials: None,
      }),
      ..ConnectionOptions::default()
    };
    let error = Conn::connect(
      "10.0.0.1:2271".parse().unwrap(),
      None,
      ConnKind::OUTBOUND | ConnKind::DIRECT,
      options,
      &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
      matches!(&error, ClientError::Timeout(message) if message == "Operation timed out after 200 milliseconds"),
      "{error:?}"
    );
    stall.abort();
  }
}
