//! Connecting and logging in: the argument checks, resolving the server, starting the listener,
//! the server connection, the login and its outcomes, and the configuration messages sent once
//! logged in.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use soulseek_proto::frame::FrameKind;
use soulseek_proto::server::{LoginRequest, LoginResponse, PrivateRoomToggle, SetListenPort};
use soulseek_proto::{LoginUsernameError, ServerCode, ServerRequest, validate_login_username};
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::Client;
use crate::conn::{Conn, ConnKind, MessageConn, ReadEvent, resolve};
use crate::error::{ClientError, ConnectionErrorKind};
use crate::inbound;
use crate::listener::Listener;
use crate::options::ClientOptions;
use crate::state::ClientState;
use crate::waiter::WaitKey;

/// The server never answers a ping, but writing one at this interval shows whether the socket
/// still takes writes.
const PING_INTERVAL: Duration = Duration::from_secs(60);

impl Client {
  /// Connects to the server at `address:port` and logs in. The listener starts first, so a busy
  /// listen port fails the connect before the server is contacted. `cancel`, or a call to
  /// [`Client::disconnect`] while this runs, ends it with `Canceled`. Any failure after the
  /// state changed leaves the client disconnected, with a `Disconnected` event naming the error.
  pub async fn connect(
    &self,
    address: &str,
    port: u16,
    username: &str,
    password: &str,
    cancel: &CancellationToken,
  ) -> Result<(), ClientError> {
    validate_arguments(address, username, password)?;
    self.inner.check_not_disposed()?;
    self.check_not_connected()?;
    let remote = tokio::select! {
      biased;
      () = cancel.cancelled() => return Err(ClientError::Canceled),
      resolved = resolve(address, port) => resolved.map_err(|error| {
        ClientError::Address(format!("Failed to resolve address '{address}': {error}"))
      })?,
    };
    let _connecting = tokio::select! {
      biased;
      () = cancel.cancelled() => return Err(ClientError::Canceled),
      guard = self.inner.state_lock.lock() => guard,
    };
    // another caller connected while this one waited for the lock
    if self.state() == ClientState::READY {
      return Ok(());
    }
    // a disconnect still tearing down the previous session finishes first
    let mut state = self.inner.state.subscribe();
    tokio::select! {
      biased;
      () = cancel.cancelled() => return Err(ClientError::Canceled),
      _ = state.wait_for(|state| !state.contains(ClientState::DISCONNECTING)) => {}
    }
    let attempt = cancel.child_token();
    {
      let mut session = self.inner.session();
      session.attempt = Some(attempt.clone());
      self.inner.transition(
        &mut session,
        ClientState::CONNECTING,
        "Connecting".into(),
        None,
      );
    }
    tracing::info!(address, port, %remote, username, "connecting to server");
    let result = self
      .connect_session(address, remote, username, password, &attempt)
      .await;
    self.inner.session().attempt = None;
    let Err(error) = result else {
      return Ok(());
    };
    let error = if attempt.is_cancelled() {
      ClientError::Canceled
    } else {
      connect_error(error)
    };
    self
      .disconnect(Some(&error.to_string()), Some(error.clone()))
      .await;
    Err(error)
  }

  fn check_not_connected(&self) -> Result<(), ClientError> {
    let state = self.state();
    if state.intersects(ClientState::CONNECTING | ClientState::LOGGING_IN) {
      return Err(ClientError::InvalidOperation(
        "A connection is already in the process of being established".into(),
      ));
    }
    if state.contains(ClientState::CONNECTED) {
      return Err(ClientError::InvalidOperation(
        "The client is already connected".into(),
      ));
    }
    Ok(())
  }

  async fn connect_session(
    &self,
    address: &str,
    remote: SocketAddr,
    username: &str,
    password: &str,
    attempt: &CancellationToken,
  ) -> Result<(), ClientError> {
    let options = self.inner.options();
    if options.enable_listener {
      let listener = self.start_listener(&options)?;
      let mut session = self.inner.session();
      self.check_attempt(attempt, ClientState::CONNECTING)?;
      session.listener = Some(listener);
    }

    let conn = Conn::connect(
      remote,
      None,
      ConnKind::OUTBOUND | ConnKind::DIRECT,
      options.effective_server_connection_options(),
      attempt,
    )
    .await?;
    let server = conn.into_message(FrameKind::Server, options.limits.frame_limits())?;
    {
      let mut session = self.inner.session();
      if let Err(error) = self.check_attempt(attempt, ClientState::CONNECTING) {
        server
          .handle()
          .disconnect(error.to_string(), Some(error.clone()));
        return Err(error);
      }
      session.server = Some(server.clone());
      session.address = Some(address.to_owned());
      session.ip_endpoint = Some(remote);
      self.inner.transition(
        &mut session,
        ClientState::CONNECTED,
        format!("Connected to {remote}"),
        None,
      );
    }
    tracing::debug!(%remote, "connected to server");
    self.start_server_reading(&server);

    self.login(username, password, &options, attempt).await?;
    self.send_configuration_messages().await?;
    spawn_ping(server);
    Ok(())
  }

  /// fails when the attempt was canceled or a disconnect moved the state on
  fn check_attempt(
    &self,
    attempt: &CancellationToken,
    expected: ClientState,
  ) -> Result<(), ClientError> {
    if attempt.is_cancelled() {
      return Err(ClientError::Canceled);
    }
    let state = self.state();
    if state != expected {
      return Err(ClientError::connection(
        ConnectionErrorKind::Other,
        format!("the server session ended while connecting (state: {state})"),
      ));
    }
    Ok(())
  }

  async fn login(
    &self,
    username: &str,
    password: &str,
    options: &ClientOptions,
    attempt: &CancellationToken,
  ) -> Result<(), ClientError> {
    {
      let mut session = self.inner.session();
      self.check_attempt(attempt, ClientState::CONNECTED)?;
      session.logging_in_as = Some(username.to_owned());
      self.inner.transition(
        &mut session,
        ClientState::CONNECTED | ClientState::LOGGING_IN,
        "Logging in".into(),
        None,
      );
    }
    let reply = self.inner.waiter.wait::<LoginResponse>(
      WaitKey::Server(ServerCode::Login),
      None,
      attempt.clone(),
    );
    // the listen port goes in the same write, so other users learn it as soon as we appear
    self
      .write_server_messages(&[
        ServerRequest::Login(LoginRequest::new(
          username.to_owned(),
          password.to_owned(),
          options.protocol_major_version,
          options.protocol_minor_version,
        )),
        ServerRequest::SetListenPort(SetListenPort {
          port: u32::from(options.listen_port),
          obfuscation: None,
        }),
      ])
      .await?;
    match reply.await {
      Ok(LoginResponse::Success { greeting, ip, .. }) => {
        // the dispatcher logged the session in as it read the reply
        self.check_attempt(attempt, ClientState::READY)?;
        tracing::info!(username, public_ip = %ip, "logged in");
        tracing::debug!(greeting, "server greeting");
        Ok(())
      }
      Ok(LoginResponse::Failure { reason, detail }) => {
        let reason = match detail {
          Some(detail) => format!("{reason}: {detail}"),
          None => reason,
        };
        Err(ClientError::LoginRejected(format!(
          "The server rejected login attempt: {reason}"
        )))
      }
      // the login was written and the connection is still up, or the disconnect would have
      // failed the wait first
      Err(ClientError::Timeout(_)) => Err(ClientError::LoginNoResponse(
        "server did not respond to login; the account may be banned".into(),
      )),
      Err(error) => Err(error),
    }
  }

  /// Tells the server our listen port, whether we take private room invitations and our place
  /// in the distributed tree; sent after login and after the options change.
  pub(super) async fn send_configuration_messages(&self) -> Result<(), ClientError> {
    let options = self.inner.options();
    self
      .write_server_message(&ServerRequest::SetListenPort(SetListenPort {
        port: u32::from(options.listen_port),
        obfuscation: None,
      }))
      .await?;
    self
      .write_server_message(&ServerRequest::PrivateRoomToggle(PrivateRoomToggle {
        accept_invitations: options.accept_private_room_invitations,
      }))
      .await?;
    self.update_distributed_status(false).await;
    Ok(())
  }

  /// Binds the listener on the configured address. Accepted connections are classified on a
  /// task of their own and handed on.
  pub(super) fn start_listener(&self, options: &ClientOptions) -> Result<Listener, ClientError> {
    let client = self.downgrade();
    Listener::start(
      SocketAddr::new(options.listen_ip_address, options.listen_port),
      options.listen_backlog,
      move |stream, remote| {
        // the client is gone; dropping the stream closes it
        let Some(client) = client.upgrade() else {
          return;
        };
        let options = client.options().incoming_connection_options.clone();
        let conn = Conn::accepted(stream, remote, options);
        let span = conn.handle().span().clone();
        let lifetime = client.cancellation().clone();
        let client = client.downgrade();
        // a disposed client drops the connection with the classification
        tokio::spawn(
          async move {
            tokio::select! {
              () = lifetime.cancelled() => {}
              () = async {
                if let Some(inbound) = inbound::classify(conn).await
                  && let Some(client) = client.upgrade()
                {
                  client.route_inbound(inbound).await;
                }
              } => {}
            }
          }
          .instrument(span),
        );
      },
    )
  }

  /// Hands every server frame to the dispatcher, and ends the session when the connection
  /// closes on its own.
  fn start_server_reading(&self, server: &MessageConn) {
    let inner = Arc::downgrade(&self.inner);
    server.start_reading(move |event| {
      if let ReadEvent::Frame(frame) = event
        && let Some(inner) = inner.upgrade()
      {
        Client { inner }.dispatch_server_frame(&frame);
      }
    });
    let inner = Arc::downgrade(&self.inner);
    let handle = server.handle().clone();
    let span = handle.span().clone();
    tokio::spawn(
      async move {
        let reason = handle.disconnected().await;
        if let Some(inner) = inner.upgrade() {
          Client { inner }
            .server_disconnected(handle.id(), reason)
            .await;
        }
      }
      .instrument(span),
    );
  }
}

/// Writes a ping every minute without waiting for an answer, so a socket that stopped taking
/// writes is noticed; stops when the connection closes.
fn spawn_ping(server: MessageConn) {
  let span = server.handle().span().clone();
  tokio::spawn(
    async move {
      let mut ticks = interval_at(Instant::now() + PING_INTERVAL, PING_INTERVAL);
      ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
      loop {
        tokio::select! {
          () = server.handle().cancellation().cancelled() => return,
          _ = ticks.tick() => {}
        }
        if let Err(error) = server.send(ServerRequest::Ping.encode()) {
          tracing::debug!(%error, "write-liveness ping not sent");
          return;
        }
        tracing::trace!("write-liveness ping sent");
      }
    }
    .instrument(span),
  );
}

fn validate_arguments(address: &str, username: &str, password: &str) -> Result<(), ClientError> {
  let invalid = |message: &str| Err(ClientError::InvalidArgument(message.into()));
  if address.trim().is_empty() {
    return invalid(
      "Address must not be a null or empty string, or one consisting only of whitespace",
    );
  }
  match validate_login_username(username) {
    Ok(()) => {}
    Err(LoginUsernameError::Empty) => {
      return invalid("Username may not be null or an empty string");
    }
    Err(LoginUsernameError::Reserved) => return invalid("Username 'server' is reserved"),
    Err(
      LoginUsernameError::TooLong
      | LoginUsernameError::InvalidCharacters
      | LoginUsernameError::EdgeSpaces,
    ) => {
      return invalid(
        "Username must be 1-30 printable ASCII characters with no leading or trailing spaces",
      );
    }
  }
  if password.is_empty() {
    return invalid("Password may not be null or an empty string");
  }
  Ok(())
}

/// what a failed connect reports: its own outcomes as they are, anything else as a connect
/// failure wrapping the cause
fn connect_error(error: ClientError) -> ClientError {
  match error {
    ClientError::LoginRejected(_)
    | ClientError::LoginNoResponse(_)
    | ClientError::Canceled
    | ClientError::Timeout(_)
    | ClientError::Listen { .. } => error,
    error => ClientError::Connect {
      message: format!("Failed to connect: {error}"),
      source: Some(Arc::new(error)),
    },
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn rejected(address: &str, username: &str, password: &str) -> String {
    match validate_arguments(address, username, password) {
      Err(ClientError::InvalidArgument(message)) => message,
      other => panic!("expected an invalid argument, got {other:?}"),
    }
  }

  #[test]
  fn arguments_are_checked_before_anything_is_sent() {
    validate_arguments("server", "alice", "secret").unwrap();
    assert!(rejected(" ", "alice", "secret").starts_with("Address must not"));
    assert_eq!(
      rejected("server", "", "secret"),
      "Username may not be null or an empty string"
    );
    assert!(rejected("server", " alice", "secret").starts_with("Username must be 1-30"));
    assert_eq!(
      rejected("server", "server", "secret"),
      "Username 'server' is reserved"
    );
    assert_eq!(
      rejected("server", "alice", ""),
      "Password may not be null or an empty string"
    );
  }
}
