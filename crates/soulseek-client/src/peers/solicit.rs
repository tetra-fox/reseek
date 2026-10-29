//! Reaching a peer two ways at once: dialling it directly, and asking the server to have it dial
//! us (a solicitation, answered by a connection that opens with `PierceFirewall` and the
//! solicitation's token). Whichever connects first is kept; the other is dropped. When a peer
//! asks the server for us instead, we dial it and open with `PierceFirewall` and the token of
//! its request.

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use soulseek_proto::init::{PeerInit, PierceFirewall};
use soulseek_proto::server::ConnectToPeerRequest;
use soulseek_proto::{InitMessage, PeerConnectionType, ServerRequest};
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
  /// a distributed connection; otherwise a message or transfer connection
  pub(crate) distributed: bool,
}

impl Solicitation {
  /// the wait the answering connection completes
  pub(crate) fn key(&self, token: u32) -> WaitKey {
    let username = self.username.clone();
    if self.distributed {
      WaitKey::SolicitedDistributedConnection { username, token }
    } else {
      WaitKey::SolicitedPeerConnection { username, token }
    }
  }
}

/// The solicitations still waiting, by token
#[derive(Default)]
pub(crate) struct Solicitations {
  pending: Mutex<HashMap<u32, Solicitation>>,
}

impl Solicitations {
  /// the solicitation `token` belongs to
  pub(crate) fn get(&self, token: u32) -> Option<Solicitation> {
    lock(&self.pending).get(&token).cloned()
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

  fn insert(&self, token: u32, solicitation: Solicitation) -> Pending<'_> {
    lock(&self.pending).insert(token, solicitation);
    Pending {
      solicitations: self,
      token,
    }
  }
}

/// removes a solicitation from the table however the wait for it ends
struct Pending<'a> {
  solicitations: &'a Solicitations,
  token: u32,
}

impl Drop for Pending<'_> {
  fn drop(&mut self) {
    lock(&self.solicitations.pending).remove(&self.token);
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

  /// What we write first on a connection made this way. One we dialled introduces us with our
  /// name, what the connection is for and token 0; one the peer dialled answers our request and
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

/// Runs both attempts and returns the first connection either makes, dropping the other; fails
/// only when both have failed. When both are ready at once the user's connection is kept: the
/// user already counts it as its connection to us, while ours has not introduced us yet.
pub(crate) async fn race(
  direct: impl Future<Output = Result<Conn, ClientError>>,
  indirect: impl Future<Output = Result<Conn, ClientError>>,
) -> Result<(Conn, Via), (ClientError, ClientError)> {
  tokio::pin!(direct, indirect);
  let mut direct_failed = None;
  let mut indirect_failed = None;
  loop {
    tokio::select! {
      biased;
      result = &mut indirect, if indirect_failed.is_none() => match result {
        Ok(conn) => return Ok((conn, Via::Indirect)),
        Err(error) => indirect_failed = Some(error),
      },
      result = &mut direct, if direct_failed.is_none() => match result {
        Ok(conn) => return Ok((conn, Via::Direct)),
        Err(error) => direct_failed = Some(error),
      },
    }
    match (direct_failed.take(), indirect_failed.take()) {
      (Some(direct), Some(indirect)) => return Err((direct, indirect)),
      (direct, indirect) => (direct_failed, indirect_failed) = (direct, indirect),
    }
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
  /// waits up to `timeout` for it to arrive, then takes the connection over with `options`. The
  /// request is forgotten however this ends.
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
      distributed: connection_type == PeerConnectionType::Distributed,
    };
    let key = solicitation.key(token);
    let _pending = self.peers().solicitations.insert(token, solicitation);
    let pierced = self
      .waiter()
      .wait::<Conn>(key, Some(timeout), cancel.clone());
    self
      .write_server_message(&ServerRequest::ConnectToPeer(ConnectToPeerRequest {
        token,
        username: username.to_owned(),
        connection_type,
      }))
      .await?;
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
  use std::future::pending;

  use super::*;

  fn failed(message: &str) -> ClientError {
    ClientError::connection(ConnectionErrorKind::Other, message)
  }

  #[tokio::test]
  async fn the_first_success_wins_even_after_a_failure() {
    let (conn, _remote) = crate::conn::tests::pair(ConnectionOptions::default()).await;
    let id = conn.handle().id();
    let result = race(async { Err(failed("refused")) }, async { Ok(conn) }).await;
    let (won, via) = result.unwrap();
    assert_eq!(won.handle().id(), id);
    assert_eq!(via, Via::Indirect);
  }

  #[tokio::test]
  async fn a_success_does_not_wait_for_the_other_attempt() {
    let (conn, _remote) = crate::conn::tests::pair(ConnectionOptions::default()).await;
    let result = race(async { Ok(conn) }, pending()).await;
    assert_eq!(result.unwrap().1, Via::Direct);
  }

  #[tokio::test]
  async fn both_failing_reports_both() {
    let result = race(async { Err(failed("direct")) }, async {
      Err(failed("indirect"))
    })
    .await;
    let (direct, indirect) = result.err().unwrap();
    assert_eq!(direct.to_string(), "direct");
    assert_eq!(indirect.to_string(), "indirect");
  }

  #[tokio::test]
  async fn both_ready_keeps_the_users_connection() {
    let (dialled, _a) = crate::conn::tests::pair(ConnectionOptions::default()).await;
    let (answered, _b) = crate::conn::tests::pair(ConnectionOptions::default()).await;
    let result = race(async { Ok(dialled) }, async { Ok(answered) }).await;
    assert_eq!(result.unwrap().1, Via::Indirect);
  }

  #[test]
  fn a_solicitation_leaves_the_table_when_its_wait_ends() {
    let solicitations = Solicitations::default();
    let pending = solicitations.insert(
      7,
      Solicitation {
        username: "bob".into(),
        distributed: false,
      },
    );
    assert!(solicitations.get(7).is_some());
    drop(pending);
    assert!(solicitations.get(7).is_none());
  }
}
