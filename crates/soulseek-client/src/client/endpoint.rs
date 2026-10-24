//! Finding where another user listens. The server knows; a bounded cache remembers each answer
//! for a minute, and lookups for one user take turns so concurrent callers share one request.
//! The per-user turn locks that nobody holds are swept every five minutes.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use soulseek_proto::server::{GetPeerAddressRequest, GetPeerAddressResponse};
use soulseek_proto::{ServerCode, ServerRequest};
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::Client;
use super::request::{failed, non_blank, user_key};
use crate::cache::BoundedCache;
use crate::error::ClientError;
use crate::lock;

/// how long an address is remembered
const REMEMBER: Duration = Duration::from_secs(60);
/// how often idle per-user locks are dropped
const LOCK_CLEANUP: Duration = Duration::from_secs(300);

/// What the client keeps for address lookups
pub(super) struct Endpoints {
  cache: Mutex<BoundedCache<String, SocketAddr>>,
  locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Endpoints {
  pub(super) fn new(capacity: u32) -> Self {
    Self {
      cache: Mutex::new(BoundedCache::new(capacity as usize, REMEMBER)),
      locks: Mutex::new(HashMap::new()),
    }
  }

  fn cached(&self, username: &str) -> Option<SocketAddr> {
    lock(&self.cache).get(&username.to_owned()).copied()
  }

  /// follows a change of the cache bound
  pub(super) fn resize(&self, capacity: u32) {
    lock(&self.cache).set_capacity(capacity as usize);
  }
}

impl Client {
  /// The address `username` listens on, from the cache or the server. A user the server
  /// reports as offline fails with `UserOffline`. Port 0 means the user is online but takes no
  /// direct connections; it is returned as it is.
  pub async fn get_user_endpoint(&self, username: &str) -> Result<SocketAddr, ClientError> {
    non_blank("username", username)?;
    let verb = format!("get the endpoint for user {username}");
    self.require_logged_in(&verb)?;
    let endpoints = &self.inner.endpoints;
    if let Some(endpoint) = endpoints.cached(username) {
      tracing::debug!(username, %endpoint, "EndPoint cache HIT");
      return Ok(endpoint);
    }
    let turn = lock(&endpoints.locks)
      .entry(username.to_owned())
      .or_default()
      .clone();
    let _turn = turn.lock().await;
    // a caller ahead of us may have just looked it up
    if let Some(endpoint) = endpoints.cached(username) {
      tracing::debug!(username, %endpoint, "EndPoint cache HIT");
      return Ok(endpoint);
    }
    let lookup_failed = |error: ClientError| match error {
      ClientError::Timeout(_) | ClientError::Cancelled => error,
      error => ClientError::UserEndPoint {
        message: format!("Failed to retrieve endpoint for user {username}: {error}"),
        source: Some(Arc::new(error)),
      },
    };
    let reply = self.waiter().wait::<GetPeerAddressResponse>(
      user_key(ServerCode::GetPeerAddress, username),
      None,
      CancellationToken::new(),
    );
    self
      .write_server_message(&ServerRequest::GetPeerAddress(GetPeerAddressRequest {
        username: username.to_owned(),
      }))
      .await
      .map_err(lookup_failed)?;
    let address = reply.await.map_err(lookup_failed)?;
    if address.ip.is_unspecified() {
      return Err(ClientError::UserOffline(format!(
        "User {username} appears to be offline"
      )));
    }
    let port = u16::try_from(address.port).map_err(|_| ClientError::UserEndPoint {
      message: format!(
        "Failed to retrieve endpoint for user {username}: the port {} is out of range",
        address.port
      ),
      source: None,
    })?;
    let endpoint = SocketAddr::from((address.ip, port));
    lock(&endpoints.cache).insert(username.to_owned(), endpoint);
    tracing::debug!(username, %endpoint, "EndPoint cache MISS");
    Ok(endpoint)
  }

  /// Makes sure we have a message connection to `username`, replacing the one we have when
  /// `invalidate_cache` is set; for warming up or refreshing a connection ahead of use.
  pub async fn connect_to_user(
    &self,
    username: &str,
    invalidate_cache: bool,
  ) -> Result<(), ClientError> {
    non_blank("username", username)?;
    let verb = format!("connect to user {username}");
    let endpoint = self.get_user_endpoint(username).await?;
    if invalidate_cache {
      self.peers().invalidate(username);
    }
    self
      .message_connection(username, Some(endpoint), None, &CancellationToken::new())
      .await
      .map(drop)
      .map_err(|error| failed(&verb, error))
  }

  /// Every five minutes, drops the per-user lookup locks nobody holds or waits for.
  pub(super) fn start_endpoint_lock_cleanup(&self) {
    let client = self.downgrade();
    let lifetime = self.cancellation().clone();
    tokio::spawn(
      async move {
        let mut ticks = interval_at(Instant::now() + LOCK_CLEANUP, LOCK_CLEANUP);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
          tokio::select! {
            () = lifetime.cancelled() => return,
            _ = ticks.tick() => {}
          }
          let Some(client) = client.upgrade() else {
            return;
          };
          let mut locks = lock(&client.inner.endpoints.locks);
          let before = locks.len();
          // a lock only the map refers to is neither held nor waited for
          locks.retain(|_, turn| Arc::strong_count(turn) > 1);
          tracing::trace!(
            dropped = before - locks.len(),
            "idle endpoint lookup locks dropped"
          );
        }
      }
      .in_current_span(),
    );
  }
}
