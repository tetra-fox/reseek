//! Reaching a peer two ways at once: dialing it directly, and asking the server to have it dial
//! us (a solicitation, answered by a connection that opens with `PierceFirewall` and the
//! solicitation's token). Whichever connects first is kept; the other is dropped. A peer may
//! still answer a solicitation after dialing won, within the time the solicitation would have
//! been waited for, so its token stays known until then. When a peer asks the server for us
//! instead, we dial it and open with `PierceFirewall` and the token of its request.

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use soulseek_proto::init::{PeerInit, PierceFirewall};
use soulseek_proto::server::ConnectToPeerRequest;
use soulseek_proto::{InitMessage, PeerConnectionType, ServerRequest};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::lock;
use crate::client::Client;
use crate::conn::{Conn, ConnKind};
use crate::error::{ClientError, ConnectionErrorKind};
use crate::options::ConnectionOptions;
use crate::waiter::WaitKey;

/// A connection request we had the server relay and wait to be answered
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Solicitation {
  pub(crate) username: String,
  /// what the connection is for
  pub(crate) connection_type: PeerConnectionType,
}

impl Solicitation {
  /// the wait the answering connection completes
  pub(crate) fn key(&self, token: u32) -> WaitKey {
    let username = self.username.clone();
    match self.connection_type {
      PeerConnectionType::Distributed => {
        WaitKey::SolicitedDistributedConnection { username, token }
      }
      PeerConnectionType::Peer | PeerConnectionType::Transfer => {
        WaitKey::SolicitedPeerConnection { username, token }
      }
    }
  }
}

/// Whether a connection answering a solicitation is still awaited
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
  /// the solicitor still waits for it
  Awaited,
  /// the solicitor stopped waiting, usually because dialing the user directly won, but the
  /// user acted on the relayed request within the time the solicitor would have waited
  Late,
}

struct Entry {
  solicitation: Solicitation,
  awaited: bool,
  /// when the solicitor would have given up waiting; an answer after this is unknown
  answerable_until: Instant,
}

/// The solicitations still waiting, and those the solicitor stopped waiting for whose answer
/// may still arrive, by token
#[derive(Default)]
pub(crate) struct Solicitations {
  pending: Mutex<HashMap<u32, Entry>>,
}

impl Solicitations {
  /// Forgets the solicitation `token` belongs to, which the user said it cannot answer, and
  /// returns it while its answer was still awaited
  pub(crate) fn refused(&self, token: u32) -> Option<Solicitation> {
    let entry = lock(&self.pending).remove(&token)?;
    entry.awaited.then_some(entry.solicitation)
  }

  /// Takes the solicitation a connection answering `token` belongs to, and whether the
  /// solicitor still waits for it. A token answered once, or past the time its solicitor would
  /// have waited, is unknown.
  pub(crate) fn take(&self, token: u32) -> Option<(Solicitation, Answer)> {
    let entry = lock(&self.pending).remove(&token)?;
    if entry.answerable_until <= Instant::now() {
      return None;
    }
    let answer = if entry.awaited {
      Answer::Awaited
    } else {
      Answer::Late
    };
    Some((entry.solicitation, answer))
  }

  /// forgets every solicitation, for when the server session that relayed them ends
  pub(crate) fn clear(&self) {
    let cleared = std::mem::take(&mut *lock(&self.pending));
    if !cleared.is_empty() {
      tracing::debug!(
        count = cleared.len(),
        "pending connection requests forgotten"
      );
    }
  }

  /// Records a solicitation whose answer is awaited for `timeout`. Solicitations whose time is
  /// up leave the table here.
  fn insert(&self, token: u32, solicitation: Solicitation, timeout: Duration) -> Pending<'_> {
    let now = Instant::now();
    let mut pending = lock(&self.pending);
    pending.retain(|_, entry| entry.awaited || entry.answerable_until > now);
    pending.insert(
      token,
      Entry {
        solicitation,
        awaited: true,
        answerable_until: now + timeout,
      },
    );
    Pending {
      solicitations: self,
      token,
    }
  }
}

/// Ends the wait for a solicitation however the wait ends. A solicitation that was not answered
/// stays known until its solicitor would have given up, so a user acting on the relayed request
/// after the solicitor stopped waiting is still recognized.
struct Pending<'a> {
  solicitations: &'a Solicitations,
  token: u32,
}

impl Drop for Pending<'_> {
  fn drop(&mut self) {
    let mut pending = lock(&self.solicitations.pending);
    let Some(entry) = pending.get_mut(&self.token) else {
      return;
    };
    if entry.answerable_until <= Instant::now() {
      pending.remove(&self.token);
    } else {
      entry.awaited = false;
    }
  }
}

/// which way a raced connection was made
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Via {
  Direct,
  Indirect,
}

impl Via {
  pub(crate) fn kind(self) -> ConnKind {
    match self {
      Self::Direct => ConnKind::OUTBOUND | ConnKind::DIRECT,
      Self::Indirect => ConnKind::OUTBOUND | ConnKind::INDIRECT,
    }
  }

  /// What we write first on a connection made this way. One we dialed introduces us with our
  /// name, what the connection is for and token 0; one the peer dialed answers our request and
  /// is known to it already.
  pub(crate) fn introduction(
    self,
    own: &str,
    connection_type: PeerConnectionType,
  ) -> Option<Vec<u8>> {
    match self {
      Self::Direct => Some(
        InitMessage::PeerInit(PeerInit {
          username: own.to_owned(),
          connection_type,
          token: 0,
        })
        .encode(),
      ),
      Self::Indirect => None,
    }
  }
}

/// Runs both attempts, built to end promptly once `stop` is canceled, and returns the first
/// connection either makes; fails only when both have failed. Once one has connected the other
/// is stopped and run to its end rather than dropped, so a connection handed to it in the same
/// instant comes back here instead of being lost. When both connected the user's connection is
/// kept: the user already counts it as its connection to us, while ours has not introduced us
/// yet.
pub(crate) async fn race(
  direct: impl Future<Output = Result<Conn, ClientError>>,
  indirect: impl Future<Output = Result<Conn, ClientError>>,
  stop: &CancellationToken,
) -> Result<(Conn, Via), (ClientError, ClientError)> {
  tokio::pin!(direct, indirect);
  let mut direct_ended = None;
  let mut indirect_ended = None;
  loop {
    let connected = matches!(direct_ended, Some(Ok(_))) || matches!(indirect_ended, Some(Ok(_)));
    if connected || (direct_ended.is_some() && indirect_ended.is_some()) {
      break;
    }
    tokio::select! {
      biased;
      ended = &mut indirect, if indirect_ended.is_none() => indirect_ended = Some(ended),
      ended = &mut direct, if direct_ended.is_none() => direct_ended = Some(ended),
    }
  }
  stop.cancel();
  let direct = match direct_ended {
    Some(ended) => ended,
    None => direct.await,
  };
  let indirect = match indirect_ended {
    Some(ended) => ended,
    None => indirect.await,
  };
  match (direct, indirect) {
    (_, Ok(conn)) => Ok((conn, Via::Indirect)),
    (Ok(conn), Err(_)) => Ok((conn, Via::Direct)),
    (Err(direct), Err(indirect)) => Err((direct, indirect)),
  }
}

impl Client {
  /// Dials a peer, holding one of the bounded connect permits while the TCP connect and any
  /// proxy handshake are under way. An address that refuses or does not answer is dropped from
  /// the endpoint cache.
  pub(crate) async fn connect_to_peer_socket(
    &self,
    remote: SocketAddr,
    username: &str,
    kind: ConnKind,
    options: ConnectionOptions,
    cancel: &CancellationToken,
  ) -> Result<Conn, ClientError> {
    let limit = self.options().limits.max_concurrent_peer_connects;
    let _permit = self.peers().connects.acquire(limit, cancel).await?;
    let connected = Conn::connect(remote, Some(username.to_owned()), kind, options, cancel).await;
    if let Err(error) = &connected {
      self.forget_unreachable(username, remote, error);
    }
    connected
  }

  /// Dials a user that asked the server to have us connect to it, and opens the connection with
  /// `PierceFirewall` and the token of that request, so the user knows which request it answers
  pub(crate) async fn pierce(
    &self,
    username: &str,
    endpoint: SocketAddr,
    token: u32,
    options: ConnectionOptions,
    cancel: &CancellationToken,
  ) -> Result<Conn, ClientError> {
    let mut conn = self
      .connect_to_peer_socket(
        endpoint,
        username,
        ConnKind::INBOUND | ConnKind::INDIRECT,
        options,
        cancel,
      )
      .await?;
    conn
      .write_all(&InitMessage::PierceFirewall(PierceFirewall { token }).encode())
      .await?;
    Ok(conn)
  }

  /// Asks the server to have `username` connect to us with a `connection_type` connection and
  /// waits up to `timeout` for it to arrive, then takes the connection over with `options`. Once
  /// `cancel` fires it ends at once, with a connection that arrived before it did. The request
  /// is forgotten however this ends.
  pub(crate) async fn solicit(
    &self,
    username: &str,
    token: u32,
    connection_type: PeerConnectionType,
    options: ConnectionOptions,
    timeout: Duration,
    cancel: &CancellationToken,
  ) -> Result<Conn, ClientError> {
    let solicitation = Solicitation {
      username: username.to_owned(),
      connection_type,
    };
    let key = solicitation.key(token);
    let _pending = self
      .peers()
      .solicitations
      .insert(token, solicitation, timeout);
    let pierced = self
      .waiter()
      .wait::<Conn>(key, Some(timeout), cancel.clone());
    let request = ServerRequest::ConnectToPeer(ConnectToPeerRequest {
      token,
      username: username.to_owned(),
      connection_type,
    });
    // a write the server is slow to take is left when the wait is canceled
    tokio::select! {
      written = self.write_server_message(&request) => written?,
      () = cancel.cancelled() => return Err(ClientError::Canceled),
    }
    tracing::debug!(
      username,
      token,
      ?connection_type,
      "connection requested through the server"
    );
    let provisional = pierced.await?;
    let stream = provisional.handoff()?;
    Conn::from_handoff(
      stream,
      Some(username.to_owned()),
      Via::Indirect.kind(),
      options,
    )
  }
}

/// the error both strategies failing comes to, naming each one's reason for the log
pub(crate) fn both_failed(
  message: String,
  (direct, indirect): (ClientError, ClientError),
) -> ClientError {
  tracing::debug!(%direct, %indirect, "direct and indirect connection attempts failed");
  ClientError::connection(ConnectionErrorKind::Other, message)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn failed(message: &str) -> ClientError {
    ClientError::connection(ConnectionErrorKind::Other, message)
  }

  /// an attempt that never connects and ends once `stop` is canceled
  async fn stopped(stop: &CancellationToken) -> Result<Conn, ClientError> {
    stop.cancelled().await;
    Err(ClientError::Canceled)
  }

  #[tokio::test]
  async fn the_first_success_wins_even_after_a_failure() {
    let (conn, _remote) = crate::conn::tests::pair(ConnectionOptions::default()).await;
    let id = conn.handle().id();
    let stop = CancellationToken::new();
    let result = race(async { Err(failed("refused")) }, async { Ok(conn) }, &stop).await;
    let (won, via) = result.unwrap();
    assert_eq!(won.handle().id(), id);
    assert_eq!(via, Via::Indirect);
  }

  #[tokio::test]
  async fn a_success_stops_the_other_attempt() {
    let (conn, _remote) = crate::conn::tests::pair(ConnectionOptions::default()).await;
    let stop = CancellationToken::new();
    let result = race(async { Ok(conn) }, stopped(&stop), &stop).await;
    assert_eq!(result.unwrap().1, Via::Direct);
    assert!(stop.is_cancelled());
  }

  #[tokio::test]
  async fn both_failing_reports_both() {
    let stop = CancellationToken::new();
    let result = race(
      async { Err(failed("direct")) },
      async { Err(failed("indirect")) },
      &stop,
    )
    .await;
    let (direct, indirect) = result.err().unwrap();
    assert_eq!(direct.to_string(), "direct");
    assert_eq!(indirect.to_string(), "indirect");
  }

  #[tokio::test]
  async fn both_ready_keeps_the_users_connection() {
    // an unordered choice would pick the dialed one in about half of these
    for _ in 0..64 {
      let (dialed, _a) = crate::conn::tests::pair(ConnectionOptions::default()).await;
      let (answered, _b) = crate::conn::tests::pair(ConnectionOptions::default()).await;
      let stop = CancellationToken::new();
      let result = race(async { Ok(dialed) }, async { Ok(answered) }, &stop).await;
      assert_eq!(result.unwrap().1, Via::Indirect);
    }
  }

  #[tokio::test]
  async fn an_answer_handed_over_as_dialing_connects_is_kept() {
    let (dialed, _a) = crate::conn::tests::pair(ConnectionOptions::default()).await;
    let (answered, _b) = crate::conn::tests::pair(ConnectionOptions::default()).await;
    let id = answered.handle().id();
    let waiter = crate::waiter::Waiter::new(Duration::from_secs(10));
    let key = bob(PeerConnectionType::Peer).key(7);
    let stop = CancellationToken::new();
    let indirect = waiter.wait::<Conn>(key.clone(), None, stop.clone());
    // the race polls the solicitation first, so the answer reaches its wait only after that
    // poll, in the same poll that our dial connects
    let direct = async {
      assert!(waiter.offer(&key, answered).is_ok(), "the wait takes it");
      Ok(dialed)
    };
    let (kept, via) = race(direct, indirect, &stop).await.unwrap();
    assert_eq!((kept.handle().id(), via), (id, Via::Indirect));
  }

  fn bob(connection_type: PeerConnectionType) -> Solicitation {
    Solicitation {
      username: "bob".into(),
      connection_type,
    }
  }

  #[tokio::test(start_paused = true)]
  async fn an_unanswered_solicitation_is_known_late_until_its_time_is_up() {
    let solicitations = Solicitations::default();
    let timeout = Duration::from_secs(10);
    drop(solicitations.insert(7, bob(PeerConnectionType::Peer), timeout));
    tokio::time::advance(Duration::from_secs(9)).await;
    assert_eq!(
      solicitations.take(7),
      Some((bob(PeerConnectionType::Peer), Answer::Late))
    );
    // answered once, it is unknown to a second answer
    assert_eq!(solicitations.take(7), None);

    drop(solicitations.insert(8, bob(PeerConnectionType::Transfer), timeout));
    tokio::time::advance(timeout).await;
    assert_eq!(solicitations.take(8), None);
  }

  #[tokio::test(start_paused = true)]
  async fn a_solicitation_is_awaited_while_its_wait_lasts() {
    let solicitations = Solicitations::default();
    let pending = solicitations.insert(7, bob(PeerConnectionType::Peer), Duration::from_secs(10));
    assert_eq!(
      solicitations.take(7),
      Some((bob(PeerConnectionType::Peer), Answer::Awaited))
    );
    // the answer took it, so the end of the wait leaves nothing behind
    drop(pending);
    assert_eq!(solicitations.take(7), None);
  }

  #[tokio::test(start_paused = true)]
  async fn a_refused_solicitation_is_not_answered_late() {
    let solicitations = Solicitations::default();
    let pending = solicitations.insert(7, bob(PeerConnectionType::Peer), Duration::from_secs(10));
    assert_eq!(
      solicitations.refused(7),
      Some(bob(PeerConnectionType::Peer))
    );
    drop(pending);
    assert_eq!(solicitations.take(7), None);
  }

  #[tokio::test(start_paused = true)]
  async fn solicitations_past_their_time_leave_the_table() {
    let solicitations = Solicitations::default();
    drop(solicitations.insert(1, bob(PeerConnectionType::Peer), Duration::from_secs(1)));
    tokio::time::advance(Duration::from_secs(2)).await;
    let _waiting = solicitations.insert(2, bob(PeerConnectionType::Peer), Duration::from_secs(1));
    assert!(!lock(&solicitations.pending).contains_key(&1));
  }
}
