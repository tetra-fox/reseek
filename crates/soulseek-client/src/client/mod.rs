//! The client: one server session at a time (connect, log in, disconnect), the listener that
//! runs alongside it, the server message dispatcher, the operations that ask the server for
//! something, and the parts that live across sessions: connections to other users, the
//! distributed search tree, the search responder, the waiter and the tokens.

mod account;
mod connect;
mod disconnect;
mod endpoint;
mod inbound;
mod interests;
mod messaging;
mod private_rooms;
mod reconfigure;
mod request;
mod rooms;
mod server;
mod users;

pub(crate) use request::{failed, non_blank};

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, Weak};

use soulseek_proto::ServerRequest;
use tokio::sync::{broadcast, watch};
use tokio_util::sync::CancellationToken;

use crate::conn::MessageConn;
use crate::distributed::DistributedNetwork;
use crate::error::ClientError;
use crate::events::{EVENT_CAPACITY, Event};
use crate::listener::Listener;
use crate::lock;
use crate::options::ClientOptions;
use crate::peers::PeerConnections;
use crate::responder::SearchResponder;
use crate::search::Searches;
use crate::state::{ClientState, ServerInfo};
use crate::token::TokenGenerator;
use crate::transfers::Transfers;
use crate::waiter::Waiter;

/// The server the library connects to when the application names none
pub const DEFAULT_SERVER_ADDRESS: &str = "server.slsknet.org";
/// The server port the library connects to when the application names none
pub const DEFAULT_SERVER_PORT: u16 = 2271;

/// A Soulseek client. Clones share one client.
#[derive(Clone)]
pub struct Client {
  inner: Arc<Inner>,
}

/// A client that does not keep it alive, for the tasks it starts
#[derive(Clone)]
pub(crate) struct WeakClient {
  inner: Weak<Inner>,
}

impl WeakClient {
  pub(crate) fn upgrade(&self) -> Option<Client> {
    self.inner.upgrade().map(|inner| Client { inner })
  }
}

struct Inner {
  /// replaced whole, so even a poisoned lock holds a complete value
  options: RwLock<Arc<ClientOptions>>,
  /// written only while `session` is locked, so transitions and their events stay in order
  state: watch::Sender<ClientState>,
  session: Mutex<Session>,
  /// serializes `connect` and `reconfigure_options`
  state_lock: tokio::sync::Mutex<()>,
  server_info: Mutex<ServerInfo>,
  waiter: Waiter,
  tokens: TokenGenerator,
  events: broadcast::Sender<Event>,
  peers: Arc<PeerConnections>,
  distributed: DistributedNetwork,
  responder: SearchResponder,
  endpoints: endpoint::Endpoints,
  transfers: Arc<Transfers>,
  searches: Searches,
  /// how many connections peers opened to our listener so far
  inbound: watch::Sender<u64>,
  /// canceled when the client is disposed or dropped
  lifetime: CancellationToken,
}

/// What exists only while a server session does
#[derive(Default)]
struct Session {
  /// the `connect` in progress, canceled by an explicit disconnect
  attempt: Option<CancellationToken>,
  listener: Option<Listener>,
  server: Option<MessageConn>,
  /// the username of the login whose reply has not arrived yet
  logging_in_as: Option<String>,
  username: Option<String>,
  address: Option<String>,
  ip_endpoint: Option<SocketAddr>,
}

impl Client {
  /// A disconnected client with `options`. Fails when the options do not validate. The client
  /// starts tasks of its own, so it must be made inside a tokio runtime; they stop when it is
  /// disposed or the last clone is dropped.
  pub fn new(options: ClientOptions) -> Result<Self, ClientError> {
    options.validate()?;
    let (events, _) = broadcast::channel(EVENT_CAPACITY);
    let (distributed, commands) = DistributedNetwork::new();
    let waiter = Waiter::new(options.message_timeout);
    let inner = Inner {
      peers: Arc::new(PeerConnections::new(
        options.limits.max_concurrent_peer_connects,
        waiter.clone(),
      )),
      waiter,
      tokens: TokenGenerator::new(options.starting_token),
      responder: SearchResponder::new(options.limits.search_response_cache_size),
      endpoints: endpoint::Endpoints::new(options.limits.user_endpoint_cache_size),
      transfers: Arc::new(Transfers::new(&options)),
      searches: Searches::new(options.maximum_concurrent_searches),
      options: RwLock::new(Arc::new(options)),
      state: watch::Sender::new(ClientState::DISCONNECTED),
      session: Mutex::new(Session::default()),
      state_lock: tokio::sync::Mutex::new(()),
      server_info: Mutex::new(ServerInfo::default()),
      events,
      distributed,
      inbound: watch::Sender::default(),
      lifetime: CancellationToken::new(),
    };
    let client = Self {
      inner: Arc::new(inner),
    };
    DistributedNetwork::run(client.downgrade(), commands);
    client.start_endpoint_lock_cleanup();
    client.start_upload_turn_cleanup();
    Ok(client)
  }

  /// where the client is in its server session
  pub fn state(&self) -> ClientState {
    *self.inner.state.borrow()
  }

  /// How many connections peers opened to our listener since the client was made, followed as
  /// they come. Every one, a peer answering a connection request we had the server relay
  /// included, shows that other users can reach the listener.
  pub fn inbound_connections(&self) -> watch::Receiver<u64> {
    self.inner.inbound.subscribe()
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

  /// every user with an established message connection, and its address
  pub fn message_connections(&self) -> Vec<(String, SocketAddr)> {
    self.inner.peers.message_connections()
  }

  /// raises `event` to every subscriber
  pub(crate) fn emit(&self, event: Event) {
    self.inner.emit(event);
  }

  /// canceled when the client is disposed or dropped; long-lived tasks stop with it
  pub fn cancellation(&self) -> &CancellationToken {
    &self.inner.lifetime
  }

  pub(crate) fn downgrade(&self) -> WeakClient {
    WeakClient {
      inner: Arc::downgrade(&self.inner),
    }
  }

  pub(crate) fn peers(&self) -> &PeerConnections {
    &self.inner.peers
  }

  pub(crate) fn peers_arc(&self) -> Arc<PeerConnections> {
    self.inner.peers.clone()
  }

  pub(crate) fn distributed(&self) -> &DistributedNetwork {
    &self.inner.distributed
  }

  pub(crate) fn responder(&self) -> &SearchResponder {
    &self.inner.responder
  }

  pub(crate) fn transfers(&self) -> &Arc<Transfers> {
    &self.inner.transfers
  }

  pub(crate) fn searches(&self) -> &Searches {
    &self.inner.searches
  }

  pub(crate) fn tokens(&self) -> &TokenGenerator {
    &self.inner.tokens
  }

  pub(crate) fn check_not_disposed(&self) -> Result<(), ClientError> {
    self.inner.check_not_disposed()
  }

  /// Our username, for the operation `verb` that needs it; fails with `InvalidOperation`
  /// naming `verb` while not logged in.
  pub fn own_username(&self, verb: &str) -> Result<String, ClientError> {
    self.require_logged_in(verb)?;
    // a disconnect between the two reads leaves no username
    self
      .username()
      .ok_or_else(|| request::not_logged_in(verb, self.state()))
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

  /// Queues a message to the server without waiting for the write, for acknowledgments,
  /// probes and reports whose failure only matters to the connection itself.
  pub(crate) fn send_server_message(&self, message: &ServerRequest) {
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

  /// Disconnects and stops the client for good: every transfer is canceled, every wait is
  /// canceled, every connection to another user is closed and every task tied to the client
  /// stops. Later calls fail with `Disposed`.
  pub async fn dispose(&self) {
    self
      .disconnect(
        Some("Client is being disposed"),
        Some(ClientError::Disposed),
      )
      .await;
    // disposed first, so the transfers this cancels do not tell their peers
    self.inner.lifetime.cancel();
    self.inner.transfers.cancel_all();
    self.inner.peers.remove_and_dispose_all();
    self.inner.waiter.cancel_all();
  }
}

impl Inner {
  fn options(&self) -> Arc<ClientOptions> {
    self
      .options
      .read()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .clone()
  }

  fn set_options(&self, options: Arc<ClientOptions>) {
    *self
      .options
      .write()
      .unwrap_or_else(std::sync::PoisonError::into_inner) = options;
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
    session: &mut Session,
    next: ClientState,
    message: String,
    error: Option<ClientError>,
  ) {
    let previous_state = self.state.send_replace(next);
    tracing::debug!(%previous_state, state = %next, message, "client state changed");
    self.emit(Event::StateChanged {
      previous_state,
      state: next,
      message: message.clone(),
      error: error.clone(),
    });
    if next == ClientState::CONNECTED {
      self.emit(Event::Connected);
    } else if next == ClientState::READY {
      let username = session
        .username
        .clone()
        .expect("a login names the session before it is ready");
      self.emit(Event::LoggedIn { username });
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
    self.peers.remove_and_dispose_all();
    let session = self
      .session
      .get_mut()
      .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(server) = session.server.take() {
      server
        .handle()
        .disconnect("Client is being disposed", Some(ClientError::Disposed));
    }
  }
}
