//! A connection's identity and life: who it is with, which way it was opened, its state, the
//! inactivity timeout, and the disconnect that happens exactly once with a reason every observer
//! sees.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

use crate::error::ClientError;
use crate::options::ConnectionOptions;

bitflags::bitflags! {
  /// Who opened a connection and whether it went through the server's relay. An established
  /// connection is one of the four `{OUTBOUND, INBOUND} | {DIRECT, INDIRECT}` pairs; an accepted
  /// connection whose first message has not been read yet has none.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct ConnKind: u8 {
    /// we connected to the remote
    const OUTBOUND = 1;
    /// the remote connected to us
    const INBOUND = 2;
    /// the connecting side dialled the other directly
    const DIRECT = 4;
    /// the connecting side dialled because the server relayed a request from the other
    const INDIRECT = 8;
  }
}

/// Where a connection is in its life. The reason travels with `Disconnected`.
#[derive(Clone, Debug)]
pub enum ConnState {
  /// made, not yet connecting
  Pending,
  /// the TCP connect or proxy handshake is in progress
  Connecting,
  /// reads and writes are possible
  Connected,
  /// the disconnect is under way
  Disconnecting,
  /// closed, for the reason given
  Disconnected(Disconnect),
}

impl fmt::Display for ConnState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(match self {
      Self::Pending => "Pending",
      Self::Connecting => "Connecting",
      Self::Connected => "Connected",
      Self::Disconnecting => "Disconnecting",
      Self::Disconnected(_) => "Disconnected",
    })
  }
}

/// Why a connection was closed
#[derive(Clone, Debug)]
pub struct Disconnect {
  pub message: String,
  pub error: Option<ClientError>,
}

/// A connection's identity in a cache: the user it belongs to, if any, and its address
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConnKey {
  pub username: Option<String>,
  pub remote: SocketAddr,
}

/// the instant of the last byte read or written
struct Activity {
  base: Instant,
  /// nanoseconds after `base`
  last: AtomicU64,
}

impl Activity {
  fn new() -> Self {
    Self {
      base: Instant::now(),
      last: AtomicU64::new(0),
    }
  }

  fn touch(&self) {
    // saturates after 584 years of connection
    let nanos = u64::try_from(self.base.elapsed().as_nanos()).unwrap_or(u64::MAX);
    self.last.store(nanos, Ordering::Relaxed);
  }

  fn last(&self) -> Instant {
    self.base + Duration::from_nanos(self.last.load(Ordering::Relaxed))
  }
}

struct Shared {
  id: Uuid,
  remote: SocketAddr,
  username: Option<String>,
  kind: ConnKind,
  options: ConnectionOptions,
  state: watch::Sender<ConnState>,
  /// cancelled when the connection is disconnected; stops its tasks and in-flight I/O
  closed: CancellationToken,
  activity: Activity,
  span: tracing::Span,
}

/// The shared view of a connection: its identity, state and disconnect. Clones are cheap and
/// all refer to the same connection.
#[derive(Clone)]
pub struct ConnHandle {
  shared: Arc<Shared>,
}

impl fmt::Debug for ConnHandle {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ConnHandle")
      .field("id", &self.shared.id)
      .field("remote", &self.shared.remote)
      .field("username", &self.shared.username)
      .field("kind", &self.shared.kind)
      .field("state", &self.state().to_string())
      .finish()
  }
}

impl ConnHandle {
  pub(super) fn new(
    remote: SocketAddr,
    username: Option<String>,
    kind: ConnKind,
    options: ConnectionOptions,
    state: ConnState,
  ) -> Self {
    let id = Uuid::new_v4();
    let span = tracing::info_span!(
      "connection",
      %id,
      %remote,
      username = username.as_deref(),
      ?kind
    );
    Self {
      shared: Arc::new(Shared {
        id,
        remote,
        username,
        kind,
        options,
        state: watch::Sender::new(state),
        closed: CancellationToken::new(),
        activity: Activity::new(),
        span,
      }),
    }
  }

  /// unique per connection object; a socket handed to a new connection gets a new id
  pub fn id(&self) -> Uuid {
    self.shared.id
  }

  /// the address of the other end
  pub fn remote(&self) -> SocketAddr {
    self.shared.remote
  }

  /// the peer this connection belongs to; `None` for the server connection and for an accepted
  /// connection not identified yet
  pub fn username(&self) -> Option<&str> {
    self.shared.username.as_deref()
  }

  /// what identifies the connection in a cache
  pub fn key(&self) -> ConnKey {
    ConnKey {
      username: self.shared.username.clone(),
      remote: self.shared.remote,
    }
  }

  /// who opened the connection and how
  pub fn kind(&self) -> ConnKind {
    self.shared.kind
  }

  /// the options the connection was made with
  pub fn options(&self) -> &ConnectionOptions {
    &self.shared.options
  }

  /// the state right now
  pub fn state(&self) -> ConnState {
    self.shared.state.borrow().clone()
  }

  /// follows the state as it changes
  pub fn watch_state(&self) -> watch::Receiver<ConnState> {
    self.shared.state.subscribe()
  }

  /// time since the last byte was read or written
  pub fn inactive_time(&self) -> Duration {
    self.shared.activity.last().elapsed()
  }

  /// cancelled once the connection is disconnected
  pub fn cancellation(&self) -> &CancellationToken {
    &self.shared.closed
  }

  /// the span the connection's log lines belong to
  pub fn span(&self) -> &tracing::Span {
    &self.shared.span
  }

  pub(super) fn is_connected(&self) -> bool {
    matches!(*self.shared.state.borrow(), ConnState::Connected)
  }

  /// Closes the connection with `message` and `error` as the reason, returning whether this
  /// call closed it. Only the first call on an open connection does anything; the reason it
  /// gives is the one every observer sees.
  pub fn disconnect(&self, message: impl Into<String>, error: Option<ClientError>) -> bool {
    let closing = self.shared.state.send_if_modified(|state| match state {
      ConnState::Pending | ConnState::Connecting | ConnState::Connected => {
        *state = ConnState::Disconnecting;
        true
      }
      ConnState::Disconnecting | ConnState::Disconnected(_) => false,
    });
    if !closing {
      return false;
    }
    let message = message.into();
    self.shared.span.in_scope(|| match &error {
      Some(error) => tracing::debug!(message, %error, "connection closed"),
      None => tracing::debug!(message, "connection closed"),
    });
    self.shared.closed.cancel();
    self
      .shared
      .state
      .send_replace(ConnState::Disconnected(Disconnect { message, error }));
    true
  }

  /// resolves with the reason once the connection is disconnected
  pub async fn disconnected(&self) -> Disconnect {
    let mut state = self.shared.state.subscribe();
    loop {
      if let ConnState::Disconnected(reason) = &*state.borrow_and_update() {
        return reason.clone();
      }
      state
        .changed()
        .await
        .expect("the state sender lives in the shared state this handle keeps alive");
    }
  }

  /// the message of the disconnect reason, for errors about I/O the disconnect interrupted
  pub(super) fn closed_message(&self) -> String {
    match &*self.shared.state.borrow() {
      ConnState::Disconnected(reason) => reason.message.clone(),
      _ => "the connection is being closed".into(),
    }
  }

  pub(super) fn not_connected(&self) -> ClientError {
    ClientError::InvalidOperation(format!(
      "Invalid attempt to send to a disconnected or transitioning connection (current state: {})",
      self.state()
    ))
  }

  /// Closes the connection once it goes `timeout` without a byte read or written.
  fn watch_inactivity(&self) {
    let Some(timeout) = self.shared.options.inactivity_timeout else {
      return;
    };
    let handle = self.clone();
    let span = self.shared.span.clone();
    tokio::spawn(
      async move {
        loop {
          let deadline = handle.shared.activity.last() + timeout;
          tokio::select! {
            () = handle.shared.closed.cancelled() => return,
            () = sleep_until(deadline) => {}
          }
          if handle.shared.activity.last() + timeout <= Instant::now() {
            let message = format!(
              "Inactivity timeout of {} milliseconds was reached",
              timeout.as_millis()
            );
            handle.disconnect(message.clone(), Some(ClientError::Timeout(message)));
            return;
          }
        }
      }
      .instrument(span),
    );
  }

  pub(super) fn connecting(&self) {
    self.shared.state.send_replace(ConnState::Connecting);
  }

  /// the connection is up: activity counts from now and the inactivity timeout starts
  pub(super) fn connected(&self) {
    self.touch();
    self.shared.state.send_replace(ConnState::Connected);
    self.watch_inactivity();
  }

  /// a byte was read or written
  pub(super) fn touch(&self) {
    self.shared.activity.touch();
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use tokio::io::AsyncWriteExt;

  use super::*;
  use crate::conn::tests::pair;

  fn timeout_options() -> ConnectionOptions {
    ConnectionOptions {
      inactivity_timeout: Some(Duration::from_secs(15)),
      ..ConnectionOptions::default()
    }
  }

  // The clock is paused only while nothing waits on the socket: a paused clock jumps to the
  // next timer whenever the runtime is idle, which can be before loopback data is seen.

  #[tokio::test]
  async fn inactivity_closes_a_silent_connection() {
    let (conn, _remote) = pair(timeout_options()).await;
    tokio::time::pause();
    tokio::time::sleep(Duration::from_millis(14_900)).await;
    assert!(matches!(conn.handle().state(), ConnState::Connected));
    let reason = conn.handle().disconnected().await;
    assert_eq!(
      reason.message,
      "Inactivity timeout of 15000 milliseconds was reached"
    );
    assert!(matches!(reason.error, Some(ClientError::Timeout(_))));
  }

  #[tokio::test]
  async fn activity_pushes_the_inactivity_deadline_back() {
    let (mut conn, mut remote) = pair(timeout_options()).await;
    tokio::time::pause();
    tokio::time::sleep(Duration::from_secs(10)).await;
    tokio::time::resume();
    remote.write_all(&[1]).await.unwrap();
    conn.read_exact(1).await.unwrap();
    tokio::time::pause();
    tokio::time::sleep(Duration::from_millis(14_900)).await;
    assert!(matches!(conn.handle().state(), ConnState::Connected));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(matches!(conn.handle().state(), ConnState::Disconnected(_)));
  }

  #[tokio::test]
  async fn no_inactivity_timeout_when_disabled() {
    let options = ConnectionOptions {
      inactivity_timeout: None,
      ..ConnectionOptions::default()
    };
    let (conn, _remote) = pair(options).await;
    tokio::time::pause();
    tokio::time::sleep(Duration::from_secs(3600)).await;
    assert!(matches!(conn.handle().state(), ConnState::Connected));
  }

  #[tokio::test]
  async fn the_first_disconnect_wins() {
    let (conn, _remote) = pair(ConnectionOptions::default()).await;
    let handle = conn.handle().clone();
    assert!(handle.disconnect("first", None));
    assert!(!handle.disconnect("second", Some(ClientError::Cancelled)));
    let reason = handle.disconnected().await;
    assert_eq!(reason.message, "first");
    assert!(reason.error.is_none());
  }
}
