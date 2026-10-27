//! The listener a client logged in to soulfind runs: an accepted connection is read until its
//! first message says what it is for, then taken by what owns that kind, or closed with the
//! reason when nothing does, and counted as a peer reaching us or answering us. The listener
//! stops with the session.

mod support;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use soulseek_client::{ClientOptions, InboundConnections};
use soulseek_proto::init::{FileTransferInit, PeerInit, PierceFirewall};
use soulseek_proto::{InitMessage, PeerConnectionType, PeerMessage};
use support::peer::PeerSide;
use support::soulfind::{self, User};
use support::{Logs, free_port};
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

fn peer_init(connection_type: PeerConnectionType) -> InitMessage {
  InitMessage::PeerInit(PeerInit {
    username: "bob".into(),
    connection_type,
    token: 0,
  })
}

/// waits for the client to close `peer`, failing if it sends anything first
async fn closed(peer: &mut TcpStream) {
  let mut rest = Vec::new();
  let read = tokio::time::timeout(Duration::from_secs(10), peer.read_to_end(&mut rest))
    .await
    .expect("the client closes the connection");
  assert_eq!(read.unwrap(), 0, "nothing is sent on a refused connection");
}

#[tokio::test]
async fn a_peer_introducing_itself_becomes_our_message_connection() {
  let Some((user, address)) = listening().await else {
    return;
  };
  // the request rides in the same write as the introduction, so the hand-over must keep it
  let mut bytes = peer_init(PeerConnectionType::Peer).encode();
  bytes.extend(PeerMessage::InfoRequest.encode());
  let mut peer = PeerSide::connect(address).await;
  peer.stream.write_all(&bytes).await.unwrap();
  let PeerMessage::InfoResponse(info) = peer.read_peer().await else {
    panic!("expected user info");
  };
  assert_eq!(info.upload_permitted, Some(1));
  let connections = user.client.message_connections();
  assert_eq!(connections.len(), 1);
  assert_eq!(connections[0].0, "bob");
  assert_eq!(connections[0].1, peer.stream.local_addr().unwrap());
}

#[tokio::test]
async fn a_transfer_connection_nothing_waits_for_is_closed() {
  let logs = Logs::capture();
  let Some((_user, address)) = listening().await else {
    return;
  };
  let mut peer = TcpStream::connect(address).await.unwrap();
  let mut bytes = peer_init(PeerConnectionType::Transfer).encode();
  bytes.extend(FileTransferInit { token: 77 }.encode());
  peer.write_all(&bytes).await.unwrap();
  closed(&mut peer).await;
  assert_eq!(
    logs
      .lines_with(&["Unexpected transfer connection", "remote_token=77"])
      .len(),
    1
  );
  assert_eq!(
    logs
      .lines_with(&[
        "connection closed",
        "Transfer connection rejected: unknown token"
      ])
      .len(),
    1
  );
}

#[tokio::test]
async fn a_child_is_refused_while_we_take_no_children() {
  let logs = Logs::capture();
  let Some((_user, address)) = listening().await else {
    return;
  };
  let mut peer = TcpStream::connect(address).await.unwrap();
  peer
    .write_all(&peer_init(PeerConnectionType::Distributed).encode())
    .await
    .unwrap();
  closed(&mut peer).await;
  assert_eq!(
    logs
      .lines_with(&["connection closed", "Not accepting children"])
      .len(),
    1
  );
}

#[tokio::test]
async fn a_pierce_answering_nothing_we_asked_for_is_closed() {
  let logs = Logs::capture();
  let Some((_user, address)) = listening().await else {
    return;
  };
  let mut peer = TcpStream::connect(address).await.unwrap();
  peer
    .write_all(&InitMessage::PierceFirewall(PierceFirewall { token: 4242 }).encode())
    .await
    .unwrap();
  closed(&mut peer).await;
  assert_eq!(
    logs
      .lines_with(&[
        "connection closed",
        "Unknown PierceFirewall attempt with token 4242"
      ])
      .len(),
    1
  );
}

#[tokio::test]
async fn peers_reaching_the_listener_are_counted_apart_from_answers() {
  let Some((user, address)) = listening().await else {
    return;
  };
  let mut counted = user.client.inbound_connections();
  assert_eq!(*counted.borrow(), InboundConnections::default());
  let mut answer = TcpStream::connect(address).await.unwrap();
  answer
    .write_all(&InitMessage::PierceFirewall(PierceFirewall { token: 4242 }).encode())
    .await
    .unwrap();
  closed(&mut answer).await;
  let mut transfer = TcpStream::connect(address).await.unwrap();
  let mut bytes = peer_init(PeerConnectionType::Transfer).encode();
  bytes.extend(FileTransferInit { token: 7 }.encode());
  transfer.write_all(&bytes).await.unwrap();
  closed(&mut transfer).await;
  let both = InboundConnections {
    direct: 1,
    answers: 1,
  };
  tokio::time::timeout(Duration::from_secs(10), counted.wait_for(|c| *c == both))
    .await
    .expect("both are counted")
    .unwrap();
}

#[tokio::test]
async fn hostile_first_messages_are_closed() {
  let Some((_user, address)) = listening().await else {
    return;
  };
  let frame = |connection_type: &str, username: &str| {
    let mut payload = vec![1];
    for text in [username, connection_type] {
      payload.extend((text.len() as u32).to_le_bytes());
      payload.extend(text.as_bytes());
    }
    payload.extend(0u32.to_le_bytes());
    let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
    frame.extend(payload);
    frame
  };
  for bytes in [
    frame("X", "bob"),
    // the reserved name and an empty one are never a peer's
    frame("P", "server"),
    frame("P", ""),
    // a first message far longer than any introduction
    u32::MAX.to_le_bytes().to_vec(),
  ] {
    let mut peer = TcpStream::connect(address).await.unwrap();
    peer.write_all(&bytes).await.unwrap();
    closed(&mut peer).await;
  }
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
