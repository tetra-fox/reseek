//! Server messages soulfind relays between two users and that the client hands on: a request to
//! connect, another user's search, and a peer's report that it could not connect.

mod support;

use soulseek_client::{Event, ServerHandoff};
use soulseek_proto::server::*;
use soulseek_proto::{PeerConnectionType, ServerRequest};
use support::options;
use support::soulfind::{self, User};

async fn handoff(user: &mut User) -> ServerHandoff {
  tokio::time::timeout(support::PATIENCE, user.handoffs.server.recv())
    .await
    .expect("the message is handed on")
    .unwrap()
}

#[tokio::test]
async fn a_relayed_connect_to_peer_is_handed_on() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("alice", options()).await;
  let mut bob = server.login("bob", options()).await;
  alice
    .send(ServerRequest::ConnectToPeer(ConnectToPeerRequest {
      token: 77,
      username: bob.username.clone(),
      connection_type: PeerConnectionType::Peer,
    }))
    .await;
  let ServerHandoff::ConnectToPeer(request) = handoff(&mut bob).await else {
    panic!("expected a connect to peer");
  };
  assert_eq!(request.username, alice.username);
  assert_eq!(request.connection_type, PeerConnectionType::Peer);
  assert_eq!(request.token, 77);
  assert_eq!(Some(request.ip), alice.client.server_info().own_public_ip);
  assert_eq!(request.port, u32::from(alice.client.options().listen_port));
}

#[tokio::test]
async fn searches_from_other_users_are_handed_on_and_our_own_are_dropped() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let mut alice = server.login("alice", options()).await;
  let bob = server.login("bob", options()).await;
  // soulfind sends a network search to everyone, the searcher included
  alice
    .send(ServerRequest::FileSearch(FileSearchRequest {
      token: 5,
      query: "our own".into(),
    }))
    .await;
  bob
    .send(ServerRequest::UserSearch(UserSearch {
      username: alice.username.clone(),
      token: 6,
      query: "theirs".into(),
    }))
    .await;
  let ServerHandoff::FileSearch(search) = handoff(&mut alice).await else {
    panic!("expected a search");
  };
  assert_eq!(
    search,
    FileSearchResponse {
      username: bob.username.clone(),
      token: 6,
      query: "theirs".into()
    }
  );
}

#[tokio::test]
async fn a_relayed_cannot_connect_is_handed_on_with_the_token_only() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let mut alice = server.login("alice", options()).await;
  let mut bob = server.login("bob", options()).await;
  alice
    .send(ServerRequest::ConnectToPeer(ConnectToPeerRequest {
      token: 78,
      username: bob.username.clone(),
      connection_type: PeerConnectionType::Peer,
    }))
    .await;
  handoff(&mut bob).await;
  bob
    .send(ServerRequest::CannotConnect(CannotConnectRequest {
      token: 78,
      username: alice.username.clone(),
    }))
    .await;
  let ServerHandoff::CannotConnect(failure) = handoff(&mut alice).await else {
    panic!("expected a cannot connect");
  };
  assert_eq!(
    failure,
    CannotConnectResponse {
      token: 78,
      username: None
    }
  );
  // the event would have been raised before the hand-off, and without a username there is none
  assert!(
    support::drain(&mut alice.events)
      .iter()
      .all(|event| !matches!(event, Event::UserCannotConnect { .. }))
  );
}
