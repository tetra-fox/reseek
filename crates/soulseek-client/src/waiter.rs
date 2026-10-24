//! Correlates requests with their replies. Most Soulseek messages carry no request id, so a
//! request registers a wait under a key built from what the reply will name (its code, a
//! username, a room, a token), and whoever reads the reply completes the oldest wait under that
//! key.

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::future::{Future, pending};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use soulseek_proto::{DistributedCode, PeerCode, RemoteFilename, ServerCode};
use tokio::sync::oneshot;
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ClientError;

/// What a reply is awaited under. The value a wait completes with is the decoded reply, or the
/// connection for the connection keys.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WaitKey {
  /// a server reply that names nothing but its code
  Server(ServerCode),
  /// a server reply about one user
  ServerUser { code: ServerCode, username: String },
  /// a server reply about one interest
  ServerInterest { code: ServerCode, interest: String },
  /// a server reply about one room
  ServerRoom { code: ServerCode, room: String },
  /// a server reply about one user in one room
  ServerRoomUser {
    code: ServerCode,
    room: String,
    username: String,
  },
  /// a peer reply that names nothing but its code
  Peer { code: PeerCode, username: String },
  /// a peer reply carrying a token
  PeerToken {
    code: PeerCode,
    username: String,
    token: u32,
  },
  /// a peer reply about one file
  PeerFile {
    code: PeerCode,
    username: String,
    filename: RemoteFilename,
  },
  /// a reply on a distributed connection
  Distributed {
    code: DistributedCode,
    username: String,
  },
  /// the first bytes of a browse response, so its progress can be followed
  BrowseResponseConnection { username: String },
  /// a peer connecting to us because the server relayed our request for a message connection
  SolicitedPeerConnection { username: String, token: u32 },
  /// a peer connecting to us because the server relayed our request for a distributed
  /// connection
  SolicitedDistributedConnection { username: String, token: u32 },
  /// an uploader connecting to us directly for a file transfer
  DirectTransfer { username: String, remote_token: u32 },
  /// an uploader's transfer connection that we made through the server's relay
  IndirectTransfer {
    username: String,
    filename: RemoteFilename,
    remote_token: u32,
  },
  /// a would-be parent's branch level
  BranchLevelMessage { connection: Uuid },
  /// a would-be parent's branch root
  BranchRootMessage { connection: Uuid },
  /// a search passed down by a would-be parent
  SearchRequestMessage { connection: Uuid },
  /// a child's subtree depth
  ChildDepthMessage {
    username: String,
    remote: SocketAddr,
  },
}

/// The group of waits that one connection's loss makes unanswerable
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
  /// answered by the server, including peers connecting to us because the server relayed our
  /// request; faulted when the server connection is lost
  Server,
  /// answered over the user's peer or distributed connection
  Peer(String),
  /// a transfer connection the user is expected to open, which outlives the other connections
  Transfer(String),
  /// answered on one connection
  Connection(Uuid),
}

impl WaitKey {
  /// whether the key belongs to `scope`
  pub fn in_scope(&self, scope: &Scope) -> bool {
    match (self, scope) {
      (
        Self::Server(_)
        | Self::ServerUser { .. }
        | Self::ServerInterest { .. }
        | Self::ServerRoom { .. }
        | Self::ServerRoomUser { .. }
        | Self::SolicitedPeerConnection { .. }
        | Self::SolicitedDistributedConnection { .. },
        Scope::Server,
      ) => true,
      (
        Self::Peer { username, .. }
        | Self::PeerToken { username, .. }
        | Self::PeerFile { username, .. }
        | Self::Distributed { username, .. }
        | Self::BrowseResponseConnection { username }
        | Self::ChildDepthMessage { username, .. },
        Scope::Peer(peer),
      ) => username == peer,
      (
        Self::DirectTransfer { username, .. } | Self::IndirectTransfer { username, .. },
        Scope::Transfer(peer),
      ) => username == peer,
      (
        Self::BranchLevelMessage { connection }
        | Self::BranchRootMessage { connection }
        | Self::SearchRequestMessage { connection },
        Scope::Connection(id),
      ) => connection == id,
      _ => false,
    }
  }
}

type Outcome = Result<Box<dyn Any + Send>, ClientError>;

struct Pending {
  id: u64,
  settle: oneshot::Sender<Outcome>,
}

struct Shared {
  default_timeout: Duration,
  next_id: AtomicU64,
  waits: Mutex<HashMap<WaitKey, VecDeque<Pending>>>,
}

impl Shared {
  fn lock(&self) -> MutexGuard<'_, HashMap<WaitKey, VecDeque<Pending>>> {
    // a panic while holding the lock cannot leave the map half-updated: every critical section
    // is a single insert, removal or drain
    self
      .waits
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner())
  }

  /// hands `outcome` to the oldest wait under `key` whose receiver is still there, or back
  /// when there is none
  fn settle(&self, key: &WaitKey, mut outcome: Outcome) -> Result<(), Outcome> {
    let mut waits = self.lock();
    let Some(queue) = waits.get_mut(key) else {
      return Err(outcome);
    };
    loop {
      let Some(pending) = queue.pop_front() else {
        waits.remove(key);
        return Err(outcome);
      };
      match pending.settle.send(outcome) {
        Ok(()) => {
          if queue.is_empty() {
            waits.remove(key);
          }
          return Ok(());
        }
        // that wait's future was dropped without removing itself yet; try the next one
        Err(returned) => outcome = returned,
      }
    }
  }

  fn remove(&self, key: &WaitKey, id: u64) {
    let mut waits = self.lock();
    if let Some(queue) = waits.get_mut(key) {
      queue.retain(|pending| pending.id != id);
      if queue.is_empty() {
        waits.remove(key);
      }
    }
  }
}

/// The table of pending waits, shared by every clone
#[derive(Clone)]
pub struct Waiter {
  shared: Arc<Shared>,
}

impl Waiter {
  /// a waiter whose waits time out after `default_timeout` unless told otherwise
  pub fn new(default_timeout: Duration) -> Self {
    Self {
      shared: Arc::new(Shared {
        default_timeout,
        next_id: AtomicU64::new(0),
        waits: Mutex::new(HashMap::new()),
      }),
    }
  }

  /// the timeout a request uses when it has no reason to pick another
  pub fn default_timeout(&self) -> Duration {
    self.shared.default_timeout
  }

  /// Registers a wait under `key` now and returns the future that resolves with the value the
  /// key is completed with. It fails with `Timeout` after `timeout`, or the default timeout when
  /// `None`, counted from now; with `Cancelled` when `cancel` fires; or with the error the key is
  /// faulted with. Dropping the future withdraws the wait. Register before sending the request,
  /// so a fast reply cannot arrive first.
  pub fn wait<T: Send + 'static>(
    &self,
    key: WaitKey,
    timeout: Option<Duration>,
    cancel: CancellationToken,
  ) -> impl Future<Output = Result<T, ClientError>> + Send + 'static {
    self.register(
      key,
      Some(timeout.unwrap_or(self.shared.default_timeout)),
      cancel,
    )
  }

  /// [`Waiter::wait`] without a timeout: it ends only when completed, faulted or cancelled
  pub fn wait_indefinitely<T: Send + 'static>(
    &self,
    key: WaitKey,
    cancel: CancellationToken,
  ) -> impl Future<Output = Result<T, ClientError>> + Send + 'static {
    self.register(key, None, cancel)
  }

  fn register<T: Send + 'static>(
    &self,
    key: WaitKey,
    timeout: Option<Duration>,
    cancel: CancellationToken,
  ) -> impl Future<Output = Result<T, ClientError>> + Send + 'static {
    let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
    let (settle, settled) = oneshot::channel();
    self
      .shared
      .lock()
      .entry(key.clone())
      .or_default()
      .push_back(Pending { id, settle });
    let deadline = timeout.map(|timeout| (Instant::now() + timeout, timeout));
    let withdraw = Withdraw {
      shared: self.shared.clone(),
      key,
      id,
    };
    async move {
      let expired = async {
        match deadline {
          Some((deadline, _)) => sleep_until(deadline).await,
          None => pending().await,
        }
      };
      let outcome = tokio::select! {
        biased;
        outcome = settled => outcome,
        () = expired => {
          let millis = deadline.map_or(0, |(_, timeout)| timeout.as_millis());
          return Err(ClientError::Timeout(format!(
            "The wait timed out after {millis} milliseconds"
          )));
        }
        () = cancel.cancelled() => return Err(ClientError::Cancelled),
      };
      match outcome {
        Ok(Ok(value)) => value.downcast::<T>().map(|value| *value).map_err(|_| {
          let key = &withdraw.key;
          debug_assert!(false, "wait under {key:?} completed with the wrong type");
          tracing::error!(?key, "wait completed with a value of the wrong type");
          ClientError::client(format!("Failed to bind wait types for key {key:?}"), None)
        }),
        Ok(Err(error)) => Err(error),
        // the waiter cancelled every wait, or was dropped
        Err(_) => Err(ClientError::Cancelled),
      }
    }
  }

  /// Completes the oldest wait under `key` with `value`, returning whether a wait took it. A
  /// value nobody waits for is dropped.
  pub fn complete<T: Send + 'static>(&self, key: &WaitKey, value: T) -> bool {
    self.offer(key, value).is_ok()
  }

  /// Completes the oldest wait under `key` with `value`, or hands `value` back when nobody
  /// waits, so a connection nobody took can be closed with a reason.
  pub(crate) fn offer<T: Send + 'static>(&self, key: &WaitKey, value: T) -> Result<(), T> {
    match self.shared.settle(key, Ok(Box::new(value))) {
      Ok(()) => Ok(()),
      Err(Ok(value)) => Err(
        *value
          .downcast::<T>()
          .expect("the value handed back is the one offered"),
      ),
      Err(Err(_)) => unreachable!("an offered value is never an error"),
    }
  }

  /// fails the oldest wait under `key` with `error`, returning whether there was one
  pub fn fault(&self, key: &WaitKey, error: ClientError) -> bool {
    self.shared.settle(key, Err(error)).is_ok()
  }

  /// whether anything waits under `key`
  pub fn has_wait(&self, key: &WaitKey) -> bool {
    self.shared.lock().contains_key(key)
  }

  /// fails every wait in `scope` with `error`, returning how many there were
  pub fn fault_scope(&self, scope: &Scope, error: &ClientError) -> usize {
    let mut waits = self.shared.lock();
    let keys: Vec<WaitKey> = waits
      .keys()
      .filter(|key| key.in_scope(scope))
      .cloned()
      .collect();
    let mut faulted = 0;
    for key in keys {
      for pending in waits.remove(&key).into_iter().flatten() {
        if pending.settle.send(Err(error.clone())).is_ok() {
          faulted += 1;
        }
      }
    }
    faulted
  }

  /// cancels every wait, for when the client shuts down
  pub fn cancel_all(&self) {
    for (_, queue) in self.shared.lock().drain() {
      for pending in queue {
        // a wait whose future is already gone needs no answer
        let _ = pending.settle.send(Err(ClientError::Cancelled));
      }
    }
  }
}

/// removes one wait from the table when its future ends or is dropped
struct Withdraw {
  shared: Arc<Shared>,
  key: WaitKey,
  id: u64,
}

impl Drop for Withdraw {
  fn drop(&mut self) {
    self.shared.remove(&self.key, self.id);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn key(username: &str) -> WaitKey {
    WaitKey::ServerUser {
      code: ServerCode::GetStatus,
      username: username.into(),
    }
  }

  fn waiter() -> Waiter {
    Waiter::new(Duration::from_secs(5))
  }

  #[tokio::test]
  async fn completions_go_to_waits_in_order() {
    let waiter = waiter();
    let first = waiter.wait::<u32>(
      key("a"),
      Some(Duration::from_secs(5)),
      CancellationToken::new(),
    );
    let second = waiter.wait::<u32>(
      key("a"),
      Some(Duration::from_secs(5)),
      CancellationToken::new(),
    );
    assert!(waiter.complete(&key("a"), 1u32));
    assert!(waiter.complete(&key("a"), 2u32));
    assert!(!waiter.complete(&key("a"), 3u32), "nobody left to take it");
    assert_eq!(first.await.unwrap(), 1);
    assert_eq!(second.await.unwrap(), 2);
    assert!(!waiter.has_wait(&key("a")));
  }

  #[tokio::test]
  async fn an_offer_nobody_takes_is_handed_back() {
    let waiter = waiter();
    assert_eq!(
      waiter.offer(&key("a"), String::from("conn")),
      Err("conn".into())
    );
    let wait = waiter.wait::<String>(key("a"), None, CancellationToken::new());
    assert_eq!(waiter.offer(&key("a"), String::from("conn")), Ok(()));
    assert_eq!(wait.await.unwrap(), "conn");
  }

  #[tokio::test]
  async fn keys_with_colons_do_not_collide() {
    let waiter = waiter();
    let room_user = |room: &str, username: &str| WaitKey::ServerRoomUser {
      code: ServerCode::PrivateRoomAddUser,
      room: room.into(),
      username: username.into(),
    };
    let wait = waiter.wait::<()>(room_user("a:b", "c"), None, CancellationToken::new());
    assert!(!waiter.complete(&room_user("a", "b:c"), ()));
    assert!(waiter.complete(&room_user("a:b", "c"), ()));
    wait.await.unwrap();
  }

  #[tokio::test(start_paused = true)]
  async fn a_timeout_withdraws_only_its_own_wait() {
    let waiter = waiter();
    let short = waiter.wait::<u32>(
      key("a"),
      Some(Duration::from_secs(1)),
      CancellationToken::new(),
    );
    let long = waiter.wait::<u32>(
      key("a"),
      Some(Duration::from_secs(10)),
      CancellationToken::new(),
    );
    assert!(matches!(
      short.await,
      Err(ClientError::Timeout(message)) if message == "The wait timed out after 1000 milliseconds"
    ));
    assert!(waiter.complete(&key("a"), 7u32));
    assert_eq!(long.await.unwrap(), 7);
  }

  #[tokio::test]
  async fn cancelling_a_newer_wait_leaves_the_older_one() {
    let waiter = waiter();
    let older = waiter.wait::<u32>(key("a"), None, CancellationToken::new());
    let cancel = CancellationToken::new();
    let newer = waiter.wait::<u32>(key("a"), None, cancel.clone());
    cancel.cancel();
    assert!(matches!(newer.await, Err(ClientError::Cancelled)));
    assert!(waiter.complete(&key("a"), 7u32));
    assert_eq!(older.await.unwrap(), 7);
  }

  #[tokio::test]
  async fn a_dropped_wait_does_not_swallow_a_completion() {
    let waiter = waiter();
    let dropped = waiter.wait::<u32>(key("a"), None, CancellationToken::new());
    let kept = waiter.wait::<u32>(key("a"), None, CancellationToken::new());
    drop(dropped);
    assert!(waiter.complete(&key("a"), 7u32));
    assert_eq!(kept.await.unwrap(), 7);
  }

  #[tokio::test(start_paused = true)]
  async fn no_timeout_means_the_default_one() {
    let waiter = waiter();
    let wait = waiter.wait::<u32>(key("a"), None, CancellationToken::new());
    assert!(matches!(
      wait.await,
      Err(ClientError::Timeout(message)) if message == "The wait timed out after 5000 milliseconds"
    ));
  }

  #[tokio::test(start_paused = true)]
  async fn an_indefinite_wait_outlasts_any_timeout() {
    let waiter = waiter();
    let wait = waiter.wait_indefinitely::<u32>(key("a"), CancellationToken::new());
    let wait = tokio::spawn(wait);
    tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
    assert!(waiter.complete(&key("a"), 7u32));
    assert_eq!(wait.await.unwrap().unwrap(), 7);
  }

  #[tokio::test(start_paused = true)]
  async fn a_zero_timeout_still_sees_a_completion_made_first() {
    let waiter = waiter();
    let wait = waiter.wait::<u32>(key("a"), Some(Duration::ZERO), CancellationToken::new());
    assert!(waiter.complete(&key("a"), 7u32));
    assert_eq!(wait.await.unwrap(), 7);
  }

  #[tokio::test]
  async fn faulting_a_scope_leaves_other_scopes() {
    let waiter = waiter();
    let server = waiter.wait::<u32>(key("a"), None, CancellationToken::new());
    let solicited = waiter.wait::<u32>(
      WaitKey::SolicitedPeerConnection {
        username: "a".into(),
        token: 1,
      },
      None,
      CancellationToken::new(),
    );
    let peer_key = WaitKey::Peer {
      code: PeerCode::InfoResponse,
      username: "a".into(),
    };
    let peer = waiter.wait::<u32>(peer_key.clone(), None, CancellationToken::new());
    let error = ClientError::connection(crate::ConnectionErrorKind::Other, "gone");
    assert_eq!(waiter.fault_scope(&Scope::Server, &error), 2);
    assert!(matches!(server.await, Err(ClientError::Connection { .. })));
    assert!(matches!(
      solicited.await,
      Err(ClientError::Connection { .. })
    ));
    assert!(waiter.has_wait(&peer_key));
    assert!(waiter.complete(&peer_key, 1u32));
    assert_eq!(peer.await.unwrap(), 1);
  }
}
