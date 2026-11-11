//! A user on soulfind whose peer side a test scripts, for what a real client does not do on cue.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use soulseek_client::ClientOptions;
use soulseek_proto::init::{FileTransferInit, PeerInit};
use soulseek_proto::{InitMessage, PeerConnectionType};
use tokio::net::TcpListener;

use super::PATIENCE;
use super::peer::PeerSide;
use super::soulfind::{Soulfind, User};
use super::transfers::content;

/// A user on soulfind whose peer side the test scripts: soulfind gives out the port of a
/// listener of the test's as the user's own.
pub struct Scripted {
  pub user: User,
  listener: TcpListener,
}

impl Scripted {
  pub async fn start(server: &Arc<Soulfind>) -> Self {
    // soulfind gives out a loopback user's address as the host's LAN address
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    let options = ClientOptions {
      enable_listener: false,
      listen_port: listener.local_addr().unwrap().port(),
      ..ClientOptions::default()
    };
    Self {
      user: server.login("peer", options).await,
      listener,
    }
  }

  pub fn name(&self) -> &str {
    &self.user.username
  }

  /// The daemon's next message connection to the peer, past its introduction.
  pub async fn connected(&self) -> PeerSide {
    let (stream, _) = tokio::time::timeout(PATIENCE, self.listener.accept())
      .await
      .expect("the daemon connects")
      .unwrap();
    let mut side = PeerSide::new(stream);
    let InitMessage::PeerInit(PeerInit {
      connection_type, ..
    }) = side.read_init().await
    else {
      panic!("the daemon introduces itself");
    };
    assert_eq!(connection_type, PeerConnectionType::Peer);
    side
  }
}

/// Connects to the daemon as `username` for the transfer `token`, reads where it asks to
/// start, checks it is `start`, and sends the file of `size` bytes from there.
pub async fn send_from(daemon: SocketAddr, username: &str, token: u32, start: u64, size: usize) {
  let mut transfer = PeerSide::connect(daemon).await;
  let mut init = InitMessage::PeerInit(PeerInit {
    username: username.into(),
    connection_type: PeerConnectionType::Transfer,
    token: 0,
  })
  .encode();
  init.extend(FileTransferInit { token }.encode());
  transfer.send(&init).await;
  let offset = u64::from_le_bytes(transfer.raw(8).await.try_into().unwrap());
  assert_eq!(offset, start);
  transfer.send(&content(size)[start as usize..]).await;
  transfer.closed().await;
}
