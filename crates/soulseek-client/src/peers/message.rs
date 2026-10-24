//! Message connections to other users: the one we open, which dials the user and asks the server
//! to have the user dial us at the same time; one a user opens to us directly, which replaces
//! whatever we had; and one we open because a user asked the server to have us connect.

use std::net::SocketAddr;

use soulseek_proto::frame::FrameKind;
use soulseek_proto::init::{PeerInit, PierceFirewall};
use soulseek_proto::server::{CannotConnectRequest, ConnectToPeerResponse};
use soulseek_proto::{InitMessage, PeerConnectionType, ServerRequest};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::solicit::{Via, both_failed, race};
use super::{Entry, Outcome, PeerConnections, lock};
use crate::client::Client;
use crate::conn::{Conn, ConnKind, MessageConn};
use crate::error::{ClientError, ConnectionErrorKind};
use crate::waiter::Scope;

/// how many times a caller follows a connection that was replaced while it waited for it
const FOLLOW_REPLACEMENTS: usize = 3;

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
    self
      .message_connection(username, endpoint, None, cancel)
      .await
  }

  /// [`Client::get_or_add_message_connection`], with the token of the request the server
  /// relays when a new attempt has to be made
  pub(crate) async fn message_connection(
    &self,
    username: &str,
    endpoint: Option<SocketAddr>,
    solicitation_token: Option<u32>,
    cancel: &CancellationToken,
  ) -> Result<MessageConn, ClientError> {
    self.check_not_disposed()?;
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
              () = cancel.cancelled() => return Err(ClientError::Cancelled),
              endpoint = self.get_user_endpoint(username) => endpoint?,
            },
          };
          self.start_outbound(username, endpoint, solicitation_token)
        }
      };
      let id = entry.id;
      let conn = tokio::select! {
        biased;
        () = cancel.cancelled() => return Err(ClientError::Cancelled),
        settled = entry.settled() => settled?,
      };
      if conn.handle().is_open() {
        return Ok(conn);
      }
      // replaced, or closed before its own watcher removed the entry; the next look finds what
      // is current
      self.peers().remove_if_current(username, id);
    }
    Err(ClientError::connection(
      ConnectionErrorKind::Other,
      format!("The message connection to {username} kept being replaced while it was established"),
    ))
  }

  /// the entry for a new outbound attempt to `username`, or the one another caller put there
  /// first
  fn start_outbound(
    &self,
    username: &str,
    endpoint: SocketAddr,
    solicitation_token: Option<u32>,
  ) -> Entry {
    let peers = self.peers();
    let mut cache = peers.cache();
    if let Some(entry) = cache.get(username) {
      return entry.clone();
    }
    PeerConnections::make_room(
      &mut cache,
      self.options().limits.max_cached_peer_connections as usize,
    );
    let (entry, settle) = peers.new_entry();
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
          result = client.establish_message_connection(&username, endpoint, solicitation_token) => result,
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

  /// dials `username` and asks the server to have the user dial us, keeping whichever connects
  /// first, then introduces us on a connection we dialled
  async fn establish_message_connection(
    &self,
    username: &str,
    endpoint: SocketAddr,
    solicitation_token: Option<u32>,
  ) -> Result<MessageConn, ClientError> {
    let own = self.own_username("establish a peer connection")?;
    let options = self.options().peer_connection_options.clone();
    let token = solicitation_token.unwrap_or_else(|| self.next_token());
    let cancel = self.cancellation().child_token();
    let direct = self.connect_to_peer_socket(
      endpoint,
      username,
      Via::Direct.kind(),
      options.clone(),
      &cancel,
    );
    let indirect = self.solicit(
      username,
      token,
      PeerConnectionType::Peer,
      options.clone(),
      options.connect_timeout,
      &cancel,
    );
    let (conn, via) = race(direct, indirect).await.map_err(|errors| {
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
    if via == Via::Direct {
      let introduce = InitMessage::PeerInit(PeerInit {
        username: own,
        connection_type: PeerConnectionType::Peer,
        token: 0,
      });
      if let Err(error) = conn.write(introduce.encode()).await {
        conn
          .handle()
          .disconnect(format!("Write error: {error}"), Some(error.clone()));
        return Err(ClientError::connection(
          ConnectionErrorKind::Other,
          format!("Failed to negotiate message connection to {username} ({endpoint}): {error}"),
        ));
      }
    }
    Ok(conn)
  }

  /// A peer connected to us directly for messages. Its connection replaces whatever we had for
  /// that user, since the peer considers this one current; the one it replaces is closed.
  pub(crate) fn add_or_update_message_connection(
    &self,
    username: &str,
    provisional: Conn,
  ) -> Result<(), ClientError> {
    self.check_not_disposed()?;
    let remote = provisional.handle().remote();
    let conn = provisional
      .handoff()
      .and_then(|stream| {
        Conn::from_handoff(
          stream,
          Some(username.to_owned()),
          ConnKind::INBOUND | ConnKind::DIRECT,
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
    {
      let mut cache = peers.cache();
      if !cache.contains_key(username) {
        PeerConnections::make_room(
          &mut cache,
          self.options().limits.max_cached_peer_connections as usize,
        );
      }
      let entry = peers.settled_entry(conn.clone());
      let replaced = cache.insert(username.to_owned(), entry.clone());
      self.watch_message_connection(username, entry.id, &conn);
      // an attempt still connecting finds it was replaced when it settles, and closes its own
      if let Some(old) = replaced.as_ref().and_then(Entry::connection) {
        old.handle().disconnect("Superseded", None);
      }
    }
    peers.cancel_inbound_indirect(username);
    self.start_peer_reading(&conn);
    tracing::debug!(username, %remote, "inbound message connection");
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
    PeerConnections::make_room(
      &mut cache,
      self.options().limits.max_cached_peer_connections as usize,
    );
    let (entry, settle) = peers.new_entry();
    cache.insert(username.clone(), entry.clone());
    drop(cache);
    let cancel = self.cancellation().child_token();
    lock(&peers.pending_inbound_indirect).insert(username.clone(), (entry.id, cancel.clone()));
    let client = self.clone();
    let token = request.token;
    let span = tracing::debug_span!("message_connection", username, %endpoint, token);
    tokio::spawn(
      async move {
        let result = client
          .pierce(&username, endpoint, token, &cancel)
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
          Err(ClientError::Cancelled) => {
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

  /// dials a user that asked for us through the server and answers with its token
  async fn pierce(
    &self,
    username: &str,
    endpoint: SocketAddr,
    token: u32,
    cancel: &CancellationToken,
  ) -> Result<Conn, ClientError> {
    let mut conn = self
      .connect_to_peer_socket(
        endpoint,
        username,
        ConnKind::INBOUND | ConnKind::INDIRECT,
        self.options().peer_connection_options.clone(),
        cancel,
      )
      .await?;
    conn
      .write_all(&InitMessage::PierceFirewall(PierceFirewall { token }).encode())
      .await?;
    Ok(conn)
  }

  /// Settles the attempt behind entry `id` with `result`. A connection whose entry was replaced
  /// meanwhile is closed; a failed attempt leaves the cache unless something replaced it.
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
        conn.handle().disconnect("Superseded", None);
      }
      Err(_) if current => {
        cache.remove(username);
      }
      Err(_) => {}
    }
    settle.send_replace(Some(result));
  }

  /// When the connection behind entry `id` closes, the entry leaves the cache if it is still
  /// current, and the waits for that user's replies fail
  fn watch_message_connection(&self, username: &str, id: u64, conn: &MessageConn) {
    let client = self.downgrade();
    let handle = conn.handle().clone();
    let username = username.to_owned();
    tokio::spawn(
      async move {
        let reason = handle.disconnected().await;
        let Some(client) = client.upgrade() else {
          return;
        };
        if !client.peers().remove_if_current(&username, id) {
          return;
        }
        tracing::debug!(
          username,
          reason = reason.message,
          "message connection closed"
        );
        let faulted = client.waiter().fault_scope(
          &Scope::Peer(username.clone()),
          &ClientError::connection(
            ConnectionErrorKind::Other,
            format!(
              "The message connection to {username} closed: {}",
              reason.message
            ),
          ),
        );
        if faulted > 0 {
          tracing::debug!(faulted, "failed the waits on the closed message connection");
        }
      }
      .instrument(conn.handle().span().clone()),
    );
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
