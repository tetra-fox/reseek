//! Message connections to other users: the one we open, which dials the user and asks the server
//! to have the user dial us at the same time; one a user opens to us, introducing itself or
//! answering our request late, which replaces whatever we had; and one we open because a user
//! asked the server to have us connect. A replaced connection is not closed: what is queued on it
//! is still written and what the user sends on it is still read and answered, until it goes idle
//! or the user closes it. Only the last two replaced per user are kept; an older one closes once
//! its queued writes are out. One address opens at most a few at once, and more are refused.

use std::net::SocketAddr;

use soulseek_proto::frame::FrameKind;
use soulseek_proto::server::{CannotConnectRequest, ConnectToPeerResponse};
use soulseek_proto::{PeerConnectionType, ServerRequest};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

use super::solicit::{Via, both_failed, race};
use super::{Entry, MESSAGE_CONNECTIONS_PER_ADDRESS, Outcome, PeerConnections, lock};
use crate::client::Client;
use crate::conn::{Conn, ConnKind, Disconnect, MessageConn};
use crate::error::{ClientError, ConnectionErrorKind};
use crate::log::LogLimiter;

/// how many times a caller follows a connection that was replaced while it waited for it
const FOLLOW_REPLACEMENTS: usize = 3;

static CROWDED_ADDRESS: LogLimiter = LogLimiter::new();

/// Why no message connection to a user could be had, and the token of the connection request
/// we had the server relay for the attempt that failed, when it was an attempt of ours
pub(crate) struct Unreached {
  pub(crate) error: ClientError,
  pub(crate) request: Option<u32>,
}

impl Client {
  /// The message connection to `username`: the cached one, the attempt already under way, or a
  /// new attempt that dials the user directly and asks the server to have the user dial us, and
  /// keeps whichever connects first. The address is looked up when not given. `cancel` stops
  /// this caller waiting; an attempt others share runs on.
  pub async fn get_or_add_message_connection(
    &self,
    username: &str,
    endpoint: Option<SocketAddr>,
    cancel: &CancellationToken,
  ) -> Result<MessageConn, ClientError> {
    self.message_connection(username, endpoint, cancel).await
  }

  /// [`Client::get_or_add_message_connection`] for the client's own requests
  pub(crate) async fn message_connection(
    &self,
    username: &str,
    endpoint: Option<SocketAddr>,
    cancel: &CancellationToken,
  ) -> Result<MessageConn, ClientError> {
    self
      .reach(username, endpoint, cancel, |_| {})
      .await
      .map_err(|unreached| unreached.error)
  }

  /// [`Client::message_connection`] that, when it fails, also names the connection request the
  /// server relayed for the attempt it waited on, which the user may still act on late. Each
  /// attempt of ours it waits on is told to `waits_on` by its request as the wait starts.
  pub(crate) async fn reach(
    &self,
    username: &str,
    endpoint: Option<SocketAddr>,
    cancel: &CancellationToken,
    mut waits_on: impl FnMut(u32),
  ) -> Result<MessageConn, Unreached> {
    let unreached = |error| Unreached {
      error,
      request: None,
    };
    self.check_not_disposed().map_err(unreached)?;
    for _ in 0..FOLLOW_REPLACEMENTS {
      let cached = self.peers().cache().get(username).cloned();
      let entry = match cached {
        Some(entry) => {
          tracing::debug!(username, "Retrieved cached message connection");
          entry
        }
        None => {
          let endpoint = match endpoint {
            Some(endpoint) => endpoint,
            None => tokio::select! {
              biased;
              () = cancel.cancelled() => return Err(unreached(ClientError::Canceled)),
              endpoint = self.get_user_endpoint(username) => endpoint.map_err(unreached)?,
            },
          };
          self.start_outbound(username, endpoint)
        }
      };
      let (id, request) = (entry.id, entry.request);
      if let Some(request) = request {
        waits_on(request);
      }
      let conn = tokio::select! {
        biased;
        () = cancel.cancelled() => return Err(unreached(ClientError::Canceled)),
        settled = entry.settled() => settled.map_err(|error| Unreached { error, request })?,
      };
      let current = PeerConnections::is_current(&self.peers().cache(), username, id);
      if current && conn.handle().is_open() {
        return Ok(conn);
      }
      // closed before its own watcher got to it: this caller does what the watcher would, so
      // the waits on the user still fail with the connection's own reason
      if current {
        let reason = conn.handle().disconnected().await;
        self.message_connection_closed(username, id, conn.handle().id(), &reason);
      }
      // the next look finds what replaced it
    }
    Err(unreached(ClientError::connection(
      ConnectionErrorKind::Other,
      format!("The message connection to {username} kept being replaced while it was established"),
    )))
  }

  /// the entry for a new outbound attempt to `username`, or the one another caller put there
  /// first
  fn start_outbound(&self, username: &str, endpoint: SocketAddr) -> Entry {
    let peers = self.peers();
    let mut cache = peers.cache();
    if let Some(entry) = cache.get(username) {
      return entry.clone();
    }
    peers.make_room(
      &mut cache,
      self.options().limits.max_cached_peer_connections as usize,
    );
    let request = self.next_token();
    let (entry, settle) = peers.new_entry(Some(request));
    cache.insert(username.to_owned(), entry.clone());
    drop(cache);
    let client = self.clone();
    let username = username.to_owned();
    let id = entry.id;
    let span = tracing::debug_span!("message_connection", username, %endpoint);
    tokio::spawn(
      async move {
        let lifetime = client.cancellation().clone();
        let result = tokio::select! {
          () = lifetime.cancelled() => Err(ClientError::Disposed),
          result = client.establish_message_connection(&username, endpoint, request) => result,
        };
        if let Err(error) = &result {
          tracing::debug!(%error, "outbound message connection failed");
        }
        client.settle(&username, id, settle, result);
      }
      .instrument(span),
    );
    entry
  }

  /// dials `username` and asks the server, with `token`, to have the user dial us, keeping
  /// whichever connects first, then introduces us on a connection we dialed
  async fn establish_message_connection(
    &self,
    username: &str,
    endpoint: SocketAddr,
    token: u32,
  ) -> Result<MessageConn, ClientError> {
    let own = self.own_username("establish a peer connection")?;
    let options = self.options().peer_connection_options.clone();
    let stop = self.cancellation().child_token();
    let direct = self.connect_to_peer_socket(
      endpoint,
      username,
      Via::Direct.kind(),
      options.clone(),
      &stop,
    );
    let indirect = self.solicit(
      username,
      token,
      PeerConnectionType::Peer,
      options.clone(),
      options.connect_timeout,
      &stop,
    );
    let (conn, via) = race(direct, indirect, &stop).await.map_err(|errors| {
      both_failed(
        format!(
          "Failed to establish a direct or indirect message connection to {username} ({endpoint})"
        ),
        errors,
      )
    })?;
    tracing::debug!(?via, "message connection established");
    let conn = self.peer_message_connection(conn)?;
    self.start_peer_reading(&conn);
    if let Some(introduction) = via.introduction(&own, PeerConnectionType::Peer)
      && let Err(error) = conn.write(introduction).await
    {
      conn
        .handle()
        .disconnect(format!("Write error: {error}"), Some(error.clone()));
      return Err(ClientError::connection(
        ConnectionErrorKind::Other,
        format!("Failed to negotiate message connection to {username} ({endpoint}): {error}"),
      ));
    }
    Ok(conn)
  }

  /// A peer connected to us for messages, introducing itself or answering a connection
  /// request of ours. Its connection replaces whatever we had for that user, since the peer
  /// considers this one current, and takes our writes from now on; the one it replaces stays
  /// open until it goes idle. `kind` says how it was made. One the remote opened is refused
  /// when its address has as many open already, so one host cannot fill the cache.
  pub(crate) fn add_or_update_message_connection(
    &self,
    username: &str,
    provisional: Conn,
    kind: ConnKind,
  ) -> Result<(), ClientError> {
    self.check_not_disposed()?;
    let remote = provisional.handle().remote();
    let conn = provisional
      .handoff()
      .and_then(|stream| {
        Conn::from_handoff(
          stream,
          Some(username.to_owned()),
          kind,
          self.options().peer_connection_options.clone(),
        )
      })
      .and_then(|conn| self.peer_message_connection(conn))
      .map_err(|error| {
        ClientError::connection(
          ConnectionErrorKind::Other,
          format!(
            "Failed to establish an inbound message connection to {username} ({remote}): {error}"
          ),
        )
      })?;
    let peers = self.peers();
    let replaced = {
      let mut cache = peers.cache();
      if kind.contains(ConnKind::INBOUND)
        && peers.opened_from(&cache, remote.ip()) >= MESSAGE_CONNECTIONS_PER_ADDRESS
      {
        drop(cache);
        if let Some(suppressed) = CROWDED_ADDRESS.allow() {
          tracing::debug!(
            username,
            %remote,
            limit = MESSAGE_CONNECTIONS_PER_ADDRESS,
            suppressed,
            "too many message connections from one address, refused"
          );
        }
        conn
          .handle()
          .disconnect("Too many message connections from this address", None);
        return Ok(());
      }
      if !cache.contains_key(username) {
        peers.make_room(
          &mut cache,
          self.options().limits.max_cached_peer_connections as usize,
        );
      }
      let entry = peers.settled_entry(conn.clone());
      let replaced = cache.insert(username.to_owned(), entry.clone());
      self.watch_message_connection(username, entry.id, &conn);
      replaced
    };
    peers.cancel_inbound_indirect(username);
    self.start_peer_reading(&conn);
    // an attempt still connecting finds it was replaced when it settles
    match replaced.as_ref().and_then(Entry::connection) {
      Some(old) => {
        tracing::debug!(
          username,
          %remote,
          ?kind,
          replaced = %old.handle().remote(),
          "message connection replaced"
        );
        peers.keep_replaced(username, old);
      }
      None => tracing::debug!(username, %remote, ?kind, "inbound message connection"),
    }
    Ok(())
  }

  /// A user asked the server to have us connect to it for messages: we dial it and answer with
  /// the token, unless we have or are making a connection to that user already. A failure is
  /// reported to the server so the user stops waiting.
  pub(crate) fn connect_to_peer_for_messages(&self, request: ConnectToPeerResponse) {
    let username = request.username;
    let Some(endpoint) = request_endpoint(request.ip, request.port) else {
      tracing::debug!(
        username,
        port = request.port,
        "connection request with an invalid port"
      );
      self.send_cannot_connect(request.token, &username);
      return;
    };
    let peers = self.peers();
    let mut cache = peers.cache();
    if cache.contains_key(&username) {
      tracing::debug!(username, "connection request ignored; a connection exists");
      return;
    }
    peers.make_room(
      &mut cache,
      self.options().limits.max_cached_peer_connections as usize,
    );
    let (entry, settle) = peers.new_entry(None);
    cache.insert(username.clone(), entry.clone());
    // under the cache lock, so a connection the user opens to us once the entry is in finds the
    // attempt to cancel
    let cancel = self.cancellation().child_token();
    lock(&peers.pending_inbound_indirect).insert(username.clone(), (entry.id, cancel.clone()));
    drop(cache);
    let client = self.clone();
    let token = request.token;
    let span = tracing::debug_span!("message_connection", username, %endpoint, token);
    tokio::spawn(
      async move {
        let options = client.options().peer_connection_options.clone();
        let result = client
          .pierce(&username, endpoint, token, options, &cancel)
          .await
          .and_then(|conn| client.peer_message_connection(conn));
        {
          let mut pending = lock(&client.peers().pending_inbound_indirect);
          if pending
            .get(&username)
            .is_some_and(|(id, _)| *id == entry.id)
          {
            pending.remove(&username);
          }
        }
        match &result {
          Ok(conn) => client.start_peer_reading(conn),
          // a direct connection from the user replaced this one
          Err(ClientError::Canceled) => {
            tracing::debug!("inbound indirect message connection superseded")
          }
          Err(error) => {
            tracing::debug!(%error, "Failed to establish an inbound indirect message connection");
            client.send_cannot_connect(token, &username);
          }
        }
        client.settle(&username, entry.id, settle, result);
      }
      .instrument(span),
    );
  }

  /// Settles the attempt behind entry `id` with `result`. A connection whose entry was replaced
  /// meanwhile is left to go idle like any replaced one; a failed attempt leaves the cache unless
  /// something replaced it.
  fn settle(
    &self,
    username: &str,
    id: u64,
    settle: watch::Sender<Outcome>,
    result: Result<MessageConn, ClientError>,
  ) {
    let peers = self.peers();
    let mut cache = peers.cache();
    let current = PeerConnections::is_current(&cache, username, id);
    match &result {
      Ok(conn) if current => self.watch_message_connection(username, id, conn),
      Ok(conn) => {
        tracing::debug!(
          username,
          "message connection made after another replaced it"
        );
        self.watch_message_connection(username, id, conn);
        peers.keep_replaced(username, conn.clone());
      }
      Err(_) if current => {
        cache.remove(username);
      }
      Err(_) => {}
    }
    settle.send_replace(Some(result));
  }

  /// Follows the connection behind entry `id` until it closes
  fn watch_message_connection(&self, username: &str, id: u64, conn: &MessageConn) {
    let client = self.downgrade();
    let handle = conn.handle().clone();
    let username = username.to_owned();
    tokio::spawn(
      async move {
        let reason = handle.disconnected().await;
        if let Some(client) = client.upgrade() {
          client.message_connection_closed(&username, id, handle.id(), &reason);
        }
      }
      .instrument(conn.handle().span().clone()),
    );
  }

  /// The connection `conn` behind entry `id` closed for `reason`: the entry leaves the cache if
  /// it is still current and the waits for the user's replies fail; a replaced one is
  /// forgotten. Its watcher and a caller that found it closed first both come here, and only
  /// the one that removes the entry fails the waits.
  fn message_connection_closed(&self, username: &str, id: u64, conn: Uuid, reason: &Disconnect) {
    // an entry that is gone was replaced, or closed with its waits failed already
    if !self.peers().remove_if_current(username, id) {
      self.peers().forget_replaced(username, conn);
      return;
    }
    tracing::debug!(
      username,
      reason = reason.message,
      "message connection closed"
    );
    self.peers().fail_waits(username, &reason.message);
  }

  /// a peer connection made into a message connection with the configured frame limits
  fn peer_message_connection(&self, conn: Conn) -> Result<MessageConn, ClientError> {
    conn.into_message(FrameKind::Peer, self.options().limits.frame_limits())
  }

  /// Tells the server we could not act on the connection request `token` from `username`, so
  /// the user stops waiting for us. Best effort.
  pub(crate) fn send_cannot_connect(&self, token: u32, username: &str) {
    self.send_server_message(&ServerRequest::CannotConnect(CannotConnectRequest {
      token,
      username: username.to_owned(),
    }));
  }
}

/// the address a `ConnectToPeer` names, unless its port does not fit
pub(crate) fn request_endpoint(ip: std::net::Ipv4Addr, port: u32) -> Option<SocketAddr> {
  let port = u16::try_from(port).ok()?;
  Some(SocketAddr::from((ip, port)))
}
