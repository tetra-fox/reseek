//! Connections to other users. Message connections (`P`) are kept one per user: whoever asks
//! for one shares the attempt already under way, a peer that connects to us replaces the one we
//! had, and an entry is only ever removed by the attempt or connection it belongs to, so an old
//! connection closing never takes a newer one with it. Transfer connections (`F`) are made per
//! file and never kept.

mod answer;
mod dispatch;
mod message;
mod solicit;
mod transfer;

pub use dispatch::IncomingBrowse;
pub(crate) use message::request_endpoint;
pub(crate) use solicit::{Solicitations, Via, both_failed, race};

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use soulseek_proto::RemoteFilename;
use tokio::sync::{Semaphore, watch};
use tokio_util::sync::CancellationToken;

use crate::conn::MessageConn;
use crate::error::{ClientError, ConnectionErrorKind};
use crate::limit::Limiter;
use crate::lock;
use crate::waiter::{Scope, Waiter};

/// peer messages handled at once across every connection; at the bound a connection's reader
/// waits for one of them to finish before reading its next frame
const PEER_HANDLERS: usize = 256;

type Outcome = Option<Result<MessageConn, ClientError>>;

/// One user's message connection, or the attempt that will produce it
#[derive(Clone)]
pub(crate) struct Entry {
  /// tells this entry apart from whatever replaces it under the same user
  id: u64,
  outcome: watch::Receiver<Outcome>,
}

impl Entry {
  /// the connection, once the attempt has produced one
  fn connection(&self) -> Option<MessageConn> {
    match &*self.outcome.borrow() {
      Some(Ok(conn)) => Some(conn.clone()),
      _ => None,
    }
  }

  /// waits for the attempt to finish
  async fn settled(mut self) -> Result<MessageConn, ClientError> {
    let outcome = self
      .outcome
      .wait_for(Option::is_some)
      .await
      .map_err(|_| ClientError::Cancelled)?;
    outcome
      .clone()
      .expect("wait_for returned once the outcome is set")
  }
}

/// What the client keeps about connections to other users
pub(crate) struct PeerConnections {
  /// one entry per user
  cache: Mutex<HashMap<String, Entry>>,
  next_entry: AtomicU64,
  /// cancels an inbound indirect attempt still connecting, so a direct connection from the same
  /// user can replace it; keyed by user, with the id of the entry the attempt belongs to
  pending_inbound_indirect: Mutex<HashMap<String, (u64, CancellationToken)>>,
  /// the connection requests we had the server relay and still wait to be answered
  pub(crate) solicitations: Solicitations,
  /// transfer connections downloads wait for an uploader to open, by user and the token the
  /// download gave the uploader
  awaited_transfers: Mutex<HashMap<(String, u32), RemoteFilename>>,
  /// TCP connects to peers in flight at once
  connects: Limiter,
  /// peer messages being handled at once
  handlers: Arc<Semaphore>,
  /// fails the waits on a user's replies once their connection is gone
  waiter: Waiter,
}

impl PeerConnections {
  pub(crate) fn new(max_concurrent_connects: u32, waiter: Waiter) -> Self {
    Self {
      cache: Mutex::new(HashMap::new()),
      next_entry: AtomicU64::new(0),
      pending_inbound_indirect: Mutex::new(HashMap::new()),
      solicitations: Solicitations::default(),
      awaited_transfers: Mutex::new(HashMap::new()),
      connects: Limiter::new(max_concurrent_connects),
      handlers: Arc::new(Semaphore::new(PEER_HANDLERS)),
      waiter,
    }
  }

  fn cache(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
    lock(&self.cache)
  }

  /// a fresh entry for an attempt, with the sender that settles it
  fn new_entry(&self) -> (Entry, watch::Sender<Outcome>) {
    let (settle, outcome) = watch::channel(None);
    let id = self.next_entry.fetch_add(1, Ordering::Relaxed);
    (Entry { id, outcome }, settle)
  }

  /// an entry already settled with `conn`
  fn settled_entry(&self, conn: MessageConn) -> Entry {
    let (entry, settle) = self.new_entry();
    settle.send_replace(Some(Ok(conn)));
    entry
  }

  /// whether `id` is still the entry the cache holds for `username`
  fn is_current(cache: &HashMap<String, Entry>, username: &str, id: u64) -> bool {
    cache.get(username).is_some_and(|entry| entry.id == id)
  }

  /// removes the entry for `username` if it is still `id`, returning whether it was
  fn remove_if_current(&self, username: &str, id: u64) -> bool {
    let mut cache = self.cache();
    let current = Self::is_current(&cache, username, id);
    if current {
      cache.remove(username);
    }
    current
  }

  /// the cached connection to `username`, once its attempt has produced one
  pub(crate) fn cached(&self, username: &str) -> Option<MessageConn> {
    self.cache().get(username).and_then(Entry::connection)
  }

  /// every user with an established message connection, and its address
  pub(crate) fn message_connections(&self) -> Vec<(String, SocketAddr)> {
    self
      .cache()
      .iter()
      .filter_map(|(username, entry)| {
        let conn = entry.connection()?;
        Some((username.clone(), conn.handle().remote()))
      })
      .collect()
  }

  /// Drops the cached connection to `username` and closes it; returns whether there was one
  pub(crate) fn invalidate(&self, username: &str) -> bool {
    let Some(entry) = self.cache().remove(username) else {
      return false;
    };
    if let Some(conn) = entry.connection() {
      self.close(username, &conn, "Invalidated");
    }
    tracing::debug!(username, "message connection invalidated");
    true
  }

  /// Closes the connection to `username` after its entry left the cache, and fails the waits
  /// on that user's replies, which it can no longer bring. Its own watcher finds the entry gone
  /// and leaves the waits alone, so ones registered for a replacement are kept.
  fn close(&self, username: &str, conn: &MessageConn, reason: &str) {
    conn.handle().disconnect(reason, None);
    self.fail_waits(username, reason);
  }

  /// fails every wait on a reply from `username`, whose connection closed for `reason`
  fn fail_waits(&self, username: &str, reason: &str) {
    let faulted = self.waiter.fault_scope(
      &Scope::Peer(username.to_owned()),
      &ClientError::connection(
        ConnectionErrorKind::Other,
        format!("The message connection to {username} closed: {reason}"),
      ),
    );
    if faulted > 0 {
      tracing::debug!(
        username,
        faulted,
        "failed the waits on the closed message connection"
      );
    }
  }

  /// Makes room for one more entry when the cache is at `capacity`, by closing the established
  /// connection that has been idle longest. Attempts still connecting are never evicted.
  fn make_room(&self, cache: &mut HashMap<String, Entry>, capacity: usize) {
    if cache.len() < capacity {
      return;
    }
    let idlest = cache
      .iter()
      .filter_map(|(username, entry)| Some((username, entry.connection()?)))
      .max_by_key(|(_, conn)| conn.handle().inactive_time())
      .map(|(username, conn)| (username.clone(), conn));
    if let Some((username, conn)) = idlest {
      cache.remove(&username);
      tracing::debug!(username, "message connection evicted from the full cache");
      self.close(&username, &conn, "Evicted");
    }
  }

  /// cancels the inbound indirect attempt still connecting to `username`, if there is one
  fn cancel_inbound_indirect(&self, username: &str) {
    if let Some((_, cancel)) = lock(&self.pending_inbound_indirect).remove(username) {
      cancel.cancel();
    }
  }

  /// Forgets every pending connection request and closes every message connection; for when
  /// the client is disposed
  pub(crate) fn remove_and_dispose_all(&self) {
    self.solicitations.clear();
    for (_, (_, cancel)) in lock(&self.pending_inbound_indirect).drain() {
      cancel.cancel();
    }
    for (_, entry) in self.cache().drain() {
      if let Some(conn) = entry.connection() {
        conn
          .handle()
          .disconnect("Client is being disposed", Some(ClientError::Disposed));
      }
    }
  }
}
