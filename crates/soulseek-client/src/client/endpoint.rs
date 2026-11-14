//! Finding where another user listens. The server knows; a bounded cache remembers each answer
//! for a minute, and lookups for one user take turns so concurrent callers share one request.
//! An address that refuses a connection or does not answer one in time is forgotten, so the next
//! lookup asks the server again. The per-user turn locks that nobody holds are swept every five
//! minutes. Whether a connection comes from where the server says a user is also asks the
//! server again when the cached address is another, since the user may have moved.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
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

  fn peek(&self, username: &str) -> Option<SocketAddr> {
    lock(&self.cache).peek(&username.to_owned()).copied()
  }

  /// the turn lookups for `username` take
  fn turn(&self, username: &str) -> Arc<tokio::sync::Mutex<()>> {
    lock(&self.locks)
      .entry(username.to_owned())
      .or_default()
      .clone()
  }

  /// follows a change of the cache bound
  pub(super) fn resize(&self, capacity: u32) {
    lock(&self.cache).set_capacity(capacity as usize);
  }

  /// forgets `username`'s address when it is still `endpoint`; a newer one stays
  fn forget(&self, username: &str, endpoint: SocketAddr) -> bool {
    let mut cache = lock(&self.cache);
    let key = username.to_owned();
    if cache.get(&key) != Some(&endpoint) {
      return false;
    }
    cache.remove(&key);
    true
  }
}

/// Whether a failed connect means nothing takes connections at that address now: it was
/// refused, or it was not answered in time.
fn unreachable(error: &ClientError) -> bool {
  match error {
    ClientError::Timeout(_) => true,
    ClientError::Connection {
      source: Some(source),
      ..
    } => {
      if let Some(io) = source.downcast_ref::<std::io::Error>() {
        return io.kind() == std::io::ErrorKind::ConnectionRefused;
      }
      source
        .downcast_ref::<ClientError>()
        .is_some_and(unreachable)
    }
    _ => false,
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
    let turn = endpoints.turn(username);
    let _turn = turn.lock().await;
    // a caller ahead of us may have just looked it up
    if let Some(endpoint) = endpoints.cached(username) {
      tracing::debug!(username, %endpoint, "EndPoint cache HIT");
      return Ok(endpoint);
    }
    self.ask_user_endpoint(username).await
  }

  /// Asks the server where `username` listens and caches the answer; for a caller holding the
  /// user's turn
  async fn ask_user_endpoint(&self, username: &str) -> Result<SocketAddr, ClientError> {
    let lookup_failed = |error: ClientError| match error {
      ClientError::Timeout(_) | ClientError::Canceled => error,
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
    lock(&self.inner.endpoints.cache).insert(username.to_owned(), endpoint);
    tracing::debug!(username, %endpoint, "EndPoint cache MISS");
    Ok(endpoint)
  }

  /// Whether the server gives `ip` as `username`'s address. A cached address that matches
  /// answers; otherwise the server is asked again, unless a caller ahead in the user's turn had
  /// it answered meanwhile. A lookup that fails is a no.
  pub(crate) async fn server_gives(&self, username: &str, ip: IpAddr) -> bool {
    let ip = ip.to_canonical();
    let matches = |endpoint: SocketAddr| endpoint.ip().to_canonical() == ip;
    let endpoints = &self.inner.endpoints;
    if endpoints.cached(username).is_some_and(matches) {
      return true;
    }
    let asked = Instant::now();
    let turn = endpoints.turn(username);
    let _turn = turn.lock().await;
    let answered = lock(&endpoints.cache)
      .stored_since(&username.to_owned(), asked)
      .copied();
    if let Some(endpoint) = answered {
      return matches(endpoint);
    }
    match self.ask_user_endpoint(username).await {
      Ok(endpoint) => matches(endpoint),
      Err(error) => {
        tracing::debug!(username, %ip, %error, "address lookup for a connection naming the user failed");
        false
      }
    }
  }

  /// The address cached for `username`, without asking the server and without counting as a
  /// use of the cache; `None` when none is cached or it expired.
  pub fn cached_user_endpoint(&self, username: &str) -> Option<SocketAddr> {
    self.inner.endpoints.peek(username)
  }

  /// Forgets the address cached for `username` when a connect to it, `endpoint`, failed with
  /// `error` because it was refused or timed out.
  pub(crate) fn forget_unreachable(
    &self,
    username: &str,
    endpoint: SocketAddr,
    error: &ClientError,
  ) {
    if unreachable(error) && self.inner.endpoints.forget(username, endpoint) {
      tracing::debug!(username, %endpoint, %error, "cached endpoint dropped after a failed connect");
    }
  }

  /// Makes sure we have a message connection to `username`, for warming one up ahead of use
  pub async fn connect_to_user(&self, username: &str) -> Result<(), ClientError> {
    non_blank("username", username)?;
    let verb = format!("connect to user {username}");
    let endpoint = self.get_user_endpoint(username).await?;
    self
      .message_connection(username, Some(endpoint), &CancellationToken::new())
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

#[cfg(test)]
mod tests {
  use std::net::TcpListener;

  use super::*;
  use crate::conn::{Conn, ConnKind};
  use crate::options::ConnectionOptions;

  #[tokio::test]
  async fn a_refused_or_unanswered_connect_makes_an_address_unreachable() {
    // a port that was just let go takes no connections
    let closed = TcpListener::bind("127.0.0.1:0")
      .unwrap()
      .local_addr()
      .unwrap();
    let refused = Conn::connect(
      closed,
      Some("alice".into()),
      ConnKind::OUTBOUND | ConnKind::DIRECT,
      ConnectionOptions::default(),
      &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(unreachable(&refused), "{refused:?}");
    assert!(unreachable(&ClientError::Timeout(
      "Operation timed out after 10000 milliseconds".into()
    )));
    assert!(!unreachable(&ClientError::Canceled));
    assert!(!unreachable(&ClientError::connection(
      crate::error::ConnectionErrorKind::Read,
      "Remote connection closed"
    )));
  }

  #[tokio::test]
  async fn a_proxy_whose_destination_refused_makes_the_address_unreachable() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = proxy.local_addr().unwrap().port();
    let refusing = tokio::spawn(async move {
      let (mut stream, _) = proxy.accept().await.unwrap();
      let mut methods = [0; 3];
      stream.read_exact(&mut methods).await.unwrap();
      stream.write_all(&[5, 0]).await.unwrap();
      let mut request = [0; 10];
      stream.read_exact(&mut request).await.unwrap();
      // reply 5: the destination refused the connection
      stream.write_all(&[5, 5, 0, 1]).await.unwrap();
    });
    let options = ConnectionOptions {
      proxy: Some(crate::options::ProxyOptions {
        address: "127.0.0.1".into(),
        port,
        credentials: None,
      }),
      ..ConnectionOptions::default()
    };
    let refused = Conn::connect(
      "198.51.100.7:2234".parse().unwrap(),
      Some("alice".into()),
      ConnKind::OUTBOUND | ConnKind::DIRECT,
      options,
      &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    refusing.await.unwrap();
    assert!(unreachable(&refused), "{refused:?}");
  }

  #[tokio::test]
  async fn an_address_answered_while_waiting_for_the_turn_is_taken_without_asking_again() {
    use std::future::Future;
    use std::task::{Context, Waker};

    // not logged in, so asking the server fails, which is a no
    let client = Client::new(crate::options::ClientOptions {
      enable_listener: false,
      ..crate::options::ClientOptions::default()
    })
    .unwrap();
    let ip = IpAddr::from([198, 51, 100, 7]);
    let endpoints = &client.inner.endpoints;
    let turn = endpoints.turn("alice");
    let ahead = turn.lock().await;
    let mut gives = Box::pin(client.server_gives("alice", ip));
    let polled = gives.as_mut().poll(&mut Context::from_waker(Waker::noop()));
    assert!(polled.is_pending(), "waits for the turn");
    // the caller ahead has the server's answer
    lock(&endpoints.cache).insert("alice".into(), SocketAddr::from((ip, 2234)));
    drop(ahead);
    assert!(gives.await);
  }

  #[test]
  fn only_the_address_that_failed_is_forgotten() {
    let endpoints = Endpoints::new(10);
    let old: SocketAddr = "198.51.100.7:2234".parse().unwrap();
    let new: SocketAddr = "198.51.100.7:2235".parse().unwrap();
    lock(&endpoints.cache).insert("alice".into(), new);
    assert!(!endpoints.forget("alice", old), "a newer address stays");
    assert_eq!(endpoints.cached("alice"), Some(new));
    assert!(endpoints.forget("alice", new));
    assert_eq!(endpoints.cached("alice"), None);
  }
}
