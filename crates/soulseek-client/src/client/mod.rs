//! The client: one server session at a time (connect, log in, disconnect), the listener that
//! runs alongside it, the server message dispatcher, and the pieces the peer, distributed and
//! search components build on (the waiter, tokens, server writes, events and hand-offs).

mod connect;
mod disconnect;
mod distributed;
mod reconfigure;
mod server;

pub use server::ServerHandoff;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use soulseek_proto::ServerRequest;
use tokio::sync::{broadcast, mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::conn::MessageConn;
use crate::error::ClientError;
use crate::events::{EVENT_CAPACITY, Event};
use crate::inbound::InboundConnection;
use crate::listener::Listener;
use crate::options::ClientOptions;
use crate::state::{ClientState, ServerInfo};
use crate::token::TokenGenerator;
use crate::waiter::Waiter;

/// The server the library connects to when the application names none
pub const DEFAULT_SERVER_ADDRESS: &str = "server.slsknet.org";
/// The server port the library connects to when the application names none
pub const DEFAULT_SERVER_PORT: u16 = 2271;

/// Server messages the client hands on without waiting for them to be taken
const SERVER_HANDOFF_CAPACITY: usize = 1024;
/// Classified inbound connections waiting to be taken
const INBOUND_CAPACITY: usize = 256;

/// The receiving ends of what the client hands on: server messages that need the peer,
/// distributed or search components, and accepted connections once their first message says
/// what they are for. A full queue drops what does not fit (closing a connection), so the
/// receivers must be drained promptly.
pub struct Handoffs {
  /// server messages for the peer, distributed and search components
  pub server: mpsc::Receiver<ServerHandoff>,
  /// accepted connections, classified by their first message
  pub inbound: mpsc::Receiver<InboundConnection>,
}

/// A Soulseek client. Clones share one client.
#[derive(Clone)]
pub struct Client {
  inner: Arc<Inner>,
}

struct Inner {
  /// replaced whole, so even a poisoned lock holds a complete value
  options: RwLock<Arc<ClientOptions>>,
  /// written only while `session` is locked, so transitions and their events stay in order
  state: watch::Sender<ClientState>,
  session: Mutex<Session>,
  /// serialises `connect` and `reconfigure_options`
  state_lock: tokio::sync::Mutex<()>,
  server_info: Mutex<ServerInfo>,
  waiter: Waiter,
  tokens: TokenGenerator,
  events: broadcast::Sender<Event>,
  server_handoffs: mpsc::Sender<ServerHandoff>,
  inbound: mpsc::Sender<InboundConnection>,
  /// the last distributed status sent to the server, to skip sending it again unchanged
  distributed_status: tokio::sync::Mutex<Option<String>>,
  /// cancelled when the client is disposed or dropped
  lifetime: CancellationToken,
}

/// What exists only while a server session does
#[derive(Default)]
struct Session {
  /// the `connect` in progress, cancelled by an explicit disconnect
  attempt: Option<CancellationToken>,
  listener: Option<Listener>,
  server: Option<MessageConn>,
  username: Option<String>,
  address: Option<String>,
  ip_endpoint: Option<SocketAddr>,
}

impl Client {
  /// A disconnected client with `options`, and the receivers of what it hands on. Fails when
  /// the options do not validate.
  pub fn new(options: ClientOptions) -> Result<(Self, Handoffs), ClientError> {
    options.validate()?;
    let (events, _) = broadcast::channel(EVENT_CAPACITY);
    let (server_handoffs, server) = mpsc::channel(SERVER_HANDOFF_CAPACITY);
    let (inbound, inbound_receiver) = mpsc::channel(INBOUND_CAPACITY);
    let inner = Inner {
      waiter: Waiter::new(options.message_timeout),
      tokens: TokenGenerator::new(options.starting_token),
      options: RwLock::new(Arc::new(options)),
      state: watch::Sender::new(ClientState::DISCONNECTED),
      session: Mutex::new(Session::default()),
      state_lock: tokio::sync::Mutex::new(()),
      server_info: Mutex::new(ServerInfo::default()),
      events,
      server_handoffs,
      inbound,
      distributed_status: tokio::sync::Mutex::new(None),
      lifetime: CancellationToken::new(),
    };
    Ok((
      Self {
        inner: Arc::new(inner),
      },
      Handoffs {
        server,
        inbound: inbound_receiver,
      },
    ))
  }

  /// where the client is in its server session
  pub fn state(&self) -> ClientState {
    *self.inner.state.borrow()
  }

  /// the server address as given to `connect`, while connected
  pub fn address(&self) -> Option<String> {
    self.inner.session().address.clone()
  }

  /// the server endpoint the address resolved to, while connected
  pub fn ip_endpoint(&self) -> Option<SocketAddr> {
    self.inner.session().ip_endpoint
  }

  /// our username, while logged in
  pub fn username(&self) -> Option<String> {
    self.inner.session().username.clone()
  }

  /// the tuning values the server has sent so far
  pub fn server_info(&self) -> ServerInfo {
    lock(&self.inner.server_info).clone()
  }

  /// the options in effect
  pub fn options(&self) -> Arc<ClientOptions> {
    self.inner.options()
  }

  /// the major version sent at login
  pub fn major_version(&self) -> u32 {
    self.inner.options().protocol_major_version
  }

  /// the minor version sent at login
  pub fn minor_version(&self) -> u32 {
    self.inner.options().protocol_minor_version
  }

  /// a receiver of every event from now on
  pub fn subscribe(&self) -> broadcast::Receiver<Event> {
    self.inner.events.subscribe()
  }

  /// the table that matches replies to the requests waiting for them
  pub fn waiter(&self) -> &Waiter {
    &self.inner.waiter
  }

  /// a fresh token for a search, a transfer or a connection request
  pub fn next_token(&self) -> u32 {
    self.inner.tokens.next()
  }

  /// cancelled when the client is disposed or dropped; long-lived tasks stop with it
  pub fn cancellation(&self) -> &CancellationToken {
    &self.inner.lifetime
  }

  /// writes one message to the server and waits until it is on the socket
  pub async fn write_server_message(&self, message: &ServerRequest) -> Result<(), ClientError> {
    self
      .write_server_messages(std::slice::from_ref(message))
      .await
  }

  /// Writes several messages to the server as one write, so they reach it together, and waits
  /// until they are on the socket.
  pub async fn write_server_messages(&self, messages: &[ServerRequest]) -> Result<(), ClientError> {
    let server = self.inner.server()?;
    let bytes = messages.iter().flat_map(ServerRequest::encode).collect();
    server.write(bytes).await?;
    for message in messages {
      tracing::debug!(
        code = message.code().code(),
        name = message.code().name(),
        "server message sent"
      );
    }
    Ok(())
  }

  /// Queues a message to the server without waiting for the write, for acknowledgements and
  /// probes whose failure only matters to the connection itself.
  fn send_server_message(&self, message: &ServerRequest) {
    let result = self
      .inner
      .server()
      .and_then(|server| server.send(message.encode()));
    let (code, name) = (message.code().code(), message.code().name());
    match result {
      Ok(()) => tracing::debug!(code, name, "server message sent"),
      Err(error) => tracing::debug!(code, name, %error, "server message not sent"),
    }
  }

  /// Disconnects and stops the client for good: every wait is cancelled and every task tied
  /// to the client stops. Later calls fail with `Disposed`.
  pub async fn dispose(&self) {
    self
      .disconnect(
        Some("Client is being disposed"),
        Some(ClientError::Disposed),
      )
      .await;
    self.inner.lifetime.cancel();
    self.inner.waiter.cancel_all();
  }
}

impl Inner {
  fn options(&self) -> Arc<ClientOptions> {
    self
      .options
      .read()
      .unwrap_or_else(PoisonError::into_inner)
      .clone()
  }

  fn set_options(&self, options: Arc<ClientOptions>) {
    *self.options.write().unwrap_or_else(PoisonError::into_inner) = options;
  }

  fn session(&self) -> MutexGuard<'_, Session> {
    lock(&self.session)
  }

  fn server(&self) -> Result<MessageConn, ClientError> {
    self.session().server.clone().ok_or_else(|| {
      ClientError::InvalidOperation(format!(
        "Invalid attempt to send to a disconnected or transitioning connection (current state: {})",
        *self.state.borrow()
      ))
    })
  }

  fn check_not_disposed(&self) -> Result<(), ClientError> {
    if self.lifetime.is_cancelled() {
      return Err(ClientError::Disposed);
    }
    Ok(())
  }

  fn emit(&self, event: Event) {
    // no subscribers is a normal state, not an error
    let _ = self.events.send(event);
  }

  /// Moves to `next` and raises its events. The caller holds the session lock, which keeps
  /// transitions and their events in one order.
  fn transition(
    &self,
    _session: &mut Session,
    next: ClientState,
    message: String,
    error: Option<ClientError>,
  ) {
    let previous = self.state.send_replace(next);
    tracing::debug!(%previous, state = %next, message, "client state changed");
    self.emit(Event::StateChanged {
      previous,
      state: next,
      message: message.clone(),
      error: error.clone(),
    });
    if next == ClientState::CONNECTED {
      self.emit(Event::Connected);
    } else if next == ClientState::READY {
      self.emit(Event::LoggedIn);
    } else if next == ClientState::DISCONNECTED {
      self.emit(Event::Disconnected { message, error });
    }
  }

  fn merge_server_info(&self, merge: impl FnOnce(&mut ServerInfo)) {
    let info = {
      let mut info = lock(&self.server_info);
      merge(&mut info);
      info.clone()
    };
    self.emit(Event::ServerInfoReceived(info));
  }
}

impl Drop for Inner {
  fn drop(&mut self) {
    self.lifetime.cancel();
    let session = self
      .session
      .get_mut()
      .unwrap_or_else(PoisonError::into_inner);
    if let Some(server) = session.server.take() {
      server
        .handle()
        .disconnect("Client is being disposed", Some(ClientError::Disposed));
    }
  }
}

/// locks a mutex whose critical sections cannot leave its data half-updated
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
