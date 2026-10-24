//! The listener a client logged in to soulfind runs: accepted connections are classified by
//! their first message and handed on, unknown ones are closed, and the listener stops with the
//! session.

mod support;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use soulseek_client::{ClientOptions, InboundConnection};
use soulseek_proto::InitMessage;
use soulseek_proto::PeerConnectionType;
use soulseek_proto::init::{PeerInit, PierceFirewall};
use support::free_port;
use support::soulfind::{self, User};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn listening() -> Option<(User, SocketAddr)> {
  let server = soulfind::server()?;
  let port = free_port();
  let user = server
    .login(
      "inbound",
      ClientOptions {
        listen_ip_address: Ipv4Addr::LOCALHOST.into(),
        listen_port: port,
        ..ClientOptions::default()
      },
    )
    .await;
  Some((user, SocketAddr::from((Ipv4Addr::LOCALHOST, port))))
}

/// connects to the listener, sends `first`, and returns what the client handed on
async fn inbound(
  user: &mut User,
  address: SocketAddr,
  first: InitMessage,
) -> (InboundConnection, TcpStream) {
  let mut peer = TcpStream::connect(address).await.unwrap();
  peer.write_all(&first.encode()).await.unwrap();
  let inbound = tokio::time::timeout(Duration::from_secs(10), user.handoffs.inbound.recv())
    .await
    .expect("the connection is handed on")
    .unwrap();
  (inbound, peer)
}

fn peer_init(connection_type: PeerConnectionType, token: u32) -> InitMessage {
  InitMessage::PeerInit(PeerInit {
    username: "bob".into(),
    connection_type,
    token,
  })
}

#[tokio::test]
async fn accepted_connections_are_handed_on_by_kind() {
  let Some((mut user, address)) = listening().await else {
    return;
  };
  let (handed, _peer) = inbound(&mut user, address, peer_init(PeerConnectionType::Peer, 0)).await;
  assert!(matches!(handed, InboundConnection::Peer { username, .. } if username == "bob"));
  let (handed, _peer) = inbound(
    &mut user,
    address,
    peer_init(PeerConnectionType::Transfer, 77),
  )
  .await;
  assert!(
    matches!(handed, InboundConnection::Transfer { username, token: 77, .. } if username == "bob")
  );
  let (handed, _peer) = inbound(
    &mut user,
    address,
    peer_init(PeerConnectionType::Distributed, 0),
  )
  .await;
  assert!(matches!(handed, InboundConnection::Distributed { username, .. } if username == "bob"));
  let (handed, _peer) = inbound(
    &mut user,
    address,
    InitMessage::PierceFirewall(PierceFirewall { token: 42 }),
  )
  .await;
  assert!(matches!(
    handed,
    InboundConnection::PierceFirewall { token: 42, .. }
  ));
}

#[tokio::test]
async fn the_handed_on_connection_carries_what_follows_the_first_message() {
  let Some((mut user, address)) = listening().await else {
    return;
  };
  let mut peer = TcpStream::connect(address).await.unwrap();
  let mut bytes = peer_init(PeerConnectionType::Transfer, 77).encode();
  bytes.extend(9u32.to_le_bytes());
  peer.write_all(&bytes).await.unwrap();
  let handed = user.handoffs.inbound.recv().await.unwrap();
  let InboundConnection::Transfer { mut conn, .. } = handed else {
    panic!("expected a transfer connection");
  };
  assert_eq!(conn.read_exact(4).await.unwrap(), 9u32.to_le_bytes());
}

#[tokio::test]
async fn an_unknown_connection_type_is_closed() {
  let Some((mut user, address)) = listening().await else {
    return;
  };
  let mut peer = TcpStream::connect(address).await.unwrap();
  let mut payload = vec![1];
  for text in ["bob", "X"] {
    payload.extend((text.len() as u32).to_le_bytes());
    payload.extend(text.as_bytes());
  }
  payload.extend(0u32.to_le_bytes());
  let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
  frame.extend(payload);
  peer.write_all(&frame).await.unwrap();
  let mut rest = Vec::new();
  let read = tokio::time::timeout(Duration::from_secs(10), peer.read_to_end(&mut rest))
    .await
    .expect("the client closes the connection");
  assert_eq!(read.unwrap(), 0);
  assert!(user.handoffs.inbound.try_recv().is_err());
}

#[tokio::test]
async fn the_listener_stops_with_the_session() {
  let Some((user, address)) = listening().await else {
    return;
  };
  TcpStream::connect(address).await.unwrap();
  user.client.disconnect(None, None).await;
  assert!(TcpStream::connect(address).await.is_err());
}
