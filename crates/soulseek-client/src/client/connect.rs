//! Connecting and logging in: the argument checks, resolving the server, starting the listener,
//! the server connection, the login and its outcomes, and the configuration messages sent once
//! logged in.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use soulseek_proto::frame::FrameKind;
use soulseek_proto::server::{LoginRequest, LoginResponse, PrivateRoomToggle, SetListenPort};
use soulseek_proto::{LoginUsernameError, ServerCode, ServerRequest, validate_login_username};
use tokio::sync::Semaphore;
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::Client;
use super::session::{Edge, Link, Stage};
use crate::conn::{Conn, ConnKind, MessageConn, ReadEvent, resolve};
use crate::error::{ClientError, ConnectionErrorKind};
use crate::inbound;
use crate::listener::Listener;
use crate::log::LogLimiter;
use crate::options::ClientOptions;
use crate::state::ClientState;
use crate::waiter::WaitKey;

/// The server never answers a ping, but writing one at this interval shows whether the socket
/// still takes writes.
const PING_INTERVAL: Duration = Duration::from_secs(60);

/// Accepted connections still waiting for their first message at once. Past it a new one is
/// closed on accept, so peers that connect and stay silent cannot use up our file descriptors.
const UNIDENTIFIED_CONNECTIONS: usize = 128;

static UNIDENTIFIED_FULL: LogLimiter = LogLimiter::new();

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
    // a disconnect still tearing down the previous session finishes first
    let mut state = self.inner.state.subscribe();
    tokio::select! {
      biased;
      () = cancel.cancelled() => return Err(ClientError::Canceled),
      _ = state.wait_for(|state| *state != ClientState::Disconnecting) => {}
    }
    let attempt = cancel.child_token();
    {
      let mut session = self.inner.session();
      // another caller connected while this one waited for the lock
      if session.stage.state().is_logged_in() {
        return Ok(());
      }
      self
        .inner
        .transition(&mut session, Edge::Connect, "Connecting".into(), None)
        .map_err(connect_refused)?;
      session.attempt = Some(attempt.clone());
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

  /// refuses a connect while a session is under way; a disconnect under way is waited for
  fn check_not_connected(&self) -> Result<(), ClientError> {
    match self.state() {
      ClientState::Disconnected | ClientState::Disconnecting => Ok(()),
      state => Err(connect_refused(state)),
    }
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
      check_canceled(attempt)?;
      let Stage::Connecting { listener: place } = &mut session.stage else {
        return Err(ended_while_connecting(session.stage.state()));
      };
      *place = Some(listener);
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
      let link = Link {
        server: server.clone(),
        address: address.to_owned(),
        ip_endpoint: remote,
      };
      let reached = check_canceled(attempt).and_then(|()| {
        self
          .inner
          .transition(
            &mut session,
            Edge::Reach(link),
            format!("Connected to {remote}"),
            None,
          )
          .map(drop)
          .map_err(ended_while_connecting)
      });
      if let Err(error) = reached {
        server
          .handle()
          .disconnect(error.to_string(), Some(error.clone()));
        return Err(error);
      }
    }
    tracing::debug!(%remote, "connected to server");
    self.start_server_reading(&server);

    self.login(username, password, &options, attempt).await?;
    self.send_configuration_messages().await?;
    spawn_ping(server);
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
      check_canceled(attempt)?;
      self
        .inner
        .transition(
          &mut session,
          Edge::LogIn(username.to_owned()),
          "Logging in".into(),
          None,
        )
        .map_err(ended_while_connecting)?;
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
        // the dispatcher logged the session in as it read the reply, unless a disconnect came
        // first
        check_canceled(attempt)?;
        let state = self.state();
        if !state.is_logged_in() {
          return Err(ended_while_connecting(state));
        }
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
  /// task of their own and handed on, at most `UNIDENTIFIED_CONNECTIONS` of them at once.
  pub(super) fn start_listener(&self, options: &ClientOptions) -> Result<Listener, ClientError> {
    let client = self.downgrade();
    let unidentified = Arc::new(Semaphore::new(UNIDENTIFIED_CONNECTIONS));
    Listener::start(
      SocketAddr::new(options.listen_ip_address, options.listen_port),
      options.listen_backlog,
      move |stream, remote| {
        // the client is gone; dropping the stream closes it
        let Some(client) = client.upgrade() else {
          return;
        };
        let Ok(classifying) = unidentified.clone().try_acquire_owned() else {
          if let Some(suppressed) = UNIDENTIFIED_FULL.allow() {
            tracing::debug!(
              %remote,
              limit = UNIDENTIFIED_CONNECTIONS,
              suppressed,
              "too many connections waiting for their first message, closed on accept"
            );
          }
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
                let inbound = inbound::classify(conn).await;
                drop(classifying);
                if let Some(inbound) = inbound
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

/// fails when the connect was canceled
fn check_canceled(attempt: &CancellationToken) -> Result<(), ClientError> {
  if attempt.is_cancelled() {
    return Err(ClientError::Canceled);
  }
  Ok(())
}

/// the error of a connect whose session a disconnect moved on to `state`
fn ended_while_connecting(state: ClientState) -> ClientError {
  ClientError::connection(
    ConnectionErrorKind::Other,
    format!("the server session ended while connecting (state: {state})"),
  )
}

/// the refusal of a connect while the session is in `state`
fn connect_refused(state: ClientState) -> ClientError {
  let message = if state.is_transitioning() {
    "A connection is already in the process of being established"
  } else {
    "The client is already connected"
  };
  ClientError::InvalidOperation(message.into())
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
