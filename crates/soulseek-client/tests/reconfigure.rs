//! Changing options while connected: the listener moves to a new port, stays where it was when
//! the new port is taken, and changes that need a new session say so.

mod support;

use std::net::{Ipv4Addr, SocketAddr};

use soulseek_client::{ClientError, ClientOptions, ClientOptionsPatch};
use soulseek_proto::ServerRequest;
use soulseek_proto::server::{
  AcceptChildren, BranchLevel, BranchRoot, HaveNoParents, PrivateRoomToggle, SetListenPort,
};
use support::{Session, USERNAME, free_port, options};
use tokio::net::TcpStream;

fn local(port: u16) -> SocketAddr {
  SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

async fn listening_on(port: u16) -> Session {
  Session::with_options(ClientOptions {
    listen_ip_address: Ipv4Addr::LOCALHOST.into(),
    listen_port: port,
    ..ClientOptions::default()
  })
  .await
}

fn listen_port(port: u16) -> ClientOptionsPatch {
  ClientOptionsPatch {
    listen_port: Some(port),
    ..ClientOptionsPatch::default()
  }
}

fn configuration(port: u16) -> [ServerRequest; 2] {
  [
    ServerRequest::SetListenPort(SetListenPort {
      port: u32::from(port),
      obfuscation: None,
    }),
    ServerRequest::PrivateRoomToggle(PrivateRoomToggle {
      accept_invitations: false,
    }),
  ]
}

#[tokio::test]
async fn the_listener_moves_to_a_new_port_and_the_server_is_told() {
  let old = free_port();
  let mut session = listening_on(old).await;
  let new = free_port();
  let reconnect = session
    .client
    .reconfigure_options(listen_port(new))
    .await
    .unwrap();
  assert!(!reconnect);
  assert!(TcpStream::connect(local(new)).await.is_ok());
  assert!(TcpStream::connect(local(old)).await.is_err());
  assert_eq!(session.client.options().listen_port, new);
  // the distributed status has not changed, so it is not sent again
  let sent = [
    session.server.request().await,
    session.server.request().await,
  ];
  assert_eq!(sent, configuration(new));
}

#[tokio::test]
async fn a_taken_port_keeps_the_old_listener_and_options() {
  let old = free_port();
  let mut session = listening_on(old).await;
  let taken = std::net::TcpListener::bind(local(0)).unwrap();
  let new = taken.local_addr().unwrap().port();
  let error = session
    .client
    .reconfigure_options(listen_port(new))
    .await
    .unwrap_err();
  assert!(matches!(error, ClientError::Listen { .. }), "{error:?}");
  assert_eq!(session.client.options().listen_port, old);
  assert!(TcpStream::connect(local(old)).await.is_ok());
  let sent = [
    session.server.request().await,
    session.server.request().await,
  ];
  assert_eq!(sent, configuration(old));
}

#[tokio::test]
async fn rebinding_the_same_port_for_a_new_backlog() {
  let port = free_port();
  let session = listening_on(port).await;
  let reconnect = session
    .client
    .reconfigure_options(ClientOptionsPatch {
      listen_backlog: Some(256),
      ..ClientOptionsPatch::default()
    })
    .await
    .unwrap();
  assert!(!reconnect);
  assert!(TcpStream::connect(local(port)).await.is_ok());
}

#[tokio::test]
async fn turning_the_distributed_network_off_needs_a_reconnect_and_on_does_not() {
  let mut session = Session::start().await;
  let off = ClientOptionsPatch {
    enable_distributed_network: Some(false),
    ..ClientOptionsPatch::default()
  };
  assert!(session.client.reconfigure_options(off).await.unwrap());
  let mut sent = Vec::new();
  for _ in 0..6 {
    sent.push(session.server.request().await);
  }
  assert_eq!(
    sent[2..],
    [
      ServerRequest::BranchLevel(BranchLevel { level: 0 }),
      ServerRequest::BranchRoot(BranchRoot {
        root: USERNAME.into()
      }),
      ServerRequest::AcceptChildren(AcceptChildren { accept: false }),
      ServerRequest::HaveNoParents(HaveNoParents {
        have_no_parents: false
      }),
    ]
  );
  let on = ClientOptionsPatch {
    enable_distributed_network: Some(true),
    ..ClientOptionsPatch::default()
  };
  assert!(!session.client.reconfigure_options(on).await.unwrap());
}

#[tokio::test]
async fn nothing_needs_a_reconnect_while_disconnected() {
  let (client, _handoffs) = soulseek_client::Client::new(options()).unwrap();
  let off = ClientOptionsPatch {
    enable_distributed_network: Some(false),
    ..ClientOptionsPatch::default()
  };
  assert!(!client.reconfigure_options(off).await.unwrap());
  assert!(!client.options().enable_distributed_network);
}
