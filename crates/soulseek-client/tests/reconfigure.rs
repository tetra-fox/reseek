//! Changing options while logged in to soulfind: the listener moves to a new port and the server
//! hands the new one out, a taken port keeps the old listener and options, and changes that need
//! a new session say so.

mod support;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use soulseek_client::{ClientError, ClientOptions, ClientOptionsPatch};
use soulseek_proto::server::{GetPeerAddressRequest, GetPeerAddressResponse};
use soulseek_proto::{ServerCode, ServerRequest};
use support::soulfind::{self, Soulfind, User};
use support::{free_port, options, user_key};
use tokio::net::TcpStream;

fn local(port: u16) -> SocketAddr {
  SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

async fn listening_on(server: &Arc<Soulfind>, port: u16) -> User {
  server
    .login(
      "moving",
      ClientOptions {
        listen_ip_address: Ipv4Addr::LOCALHOST.into(),
        listen_port: port,
        ..ClientOptions::default()
      },
    )
    .await
}

fn listen_port(port: u16) -> ClientOptionsPatch {
  ClientOptionsPatch {
    listen_port: Some(port),
    ..ClientOptionsPatch::default()
  }
}

/// the port the server gives out for `user`, as another user asks for it
async fn advertised_port(server: &Arc<Soulfind>, user: &User) -> u32 {
  let asker = server.login("asker", options()).await;
  let address: GetPeerAddressResponse = asker
    .ask(
      user_key(ServerCode::GetPeerAddress, &user.username),
      ServerRequest::GetPeerAddress(GetPeerAddressRequest {
        username: user.username.clone(),
      }),
    )
    .await;
  address.port
}

#[tokio::test]
async fn the_listener_moves_to_a_new_port_and_the_server_hands_it_out() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let old = free_port();
  let user = listening_on(&server, old).await;
  let new = free_port();
  let reconnect = user
    .client
    .reconfigure_options(listen_port(new))
    .await
    .unwrap();
  assert!(!reconnect);
  assert!(TcpStream::connect(local(new)).await.is_ok());
  assert!(TcpStream::connect(local(old)).await.is_err());
  assert_eq!(user.client.options().listen_port, new);
  assert_eq!(advertised_port(&server, &user).await, u32::from(new));
}

#[tokio::test]
async fn a_taken_port_keeps_the_old_listener_and_options() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let old = free_port();
  let user = listening_on(&server, old).await;
  let taken = std::net::TcpListener::bind(local(0)).unwrap();
  let new = taken.local_addr().unwrap().port();
  let error = user
    .client
    .reconfigure_options(listen_port(new))
    .await
    .unwrap_err();
  assert!(matches!(error, ClientError::Listen { .. }), "{error:?}");
  assert_eq!(user.client.options().listen_port, old);
  assert!(TcpStream::connect(local(old)).await.is_ok());
  assert_eq!(advertised_port(&server, &user).await, u32::from(old));
}

#[tokio::test]
async fn rebinding_the_same_port_for_a_new_backlog() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let port = free_port();
  let user = listening_on(&server, port).await;
  let reconnect = user
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
  let Some(server) = soulfind::server() else {
    return;
  };
  let user = server.login("distributed", options()).await;
  let off = ClientOptionsPatch {
    enable_distributed_network: Some(false),
    ..ClientOptionsPatch::default()
  };
  assert!(user.client.reconfigure_options(off).await.unwrap());
  let on = ClientOptionsPatch {
    enable_distributed_network: Some(true),
    ..ClientOptionsPatch::default()
  };
  assert!(!user.client.reconfigure_options(on).await.unwrap());
}

#[tokio::test]
async fn nothing_needs_a_reconnect_while_disconnected() {
  let client = soulseek_client::Client::new(options()).unwrap();
  let off = ClientOptionsPatch {
    enable_distributed_network: Some(false),
    ..ClientOptionsPatch::default()
  };
  assert!(!client.reconfigure_options(off).await.unwrap());
  assert!(!client.options().enable_distributed_network);
}
