//! Whether other users can connect to us, judged by a daemon on the scripted server: a peer
//! connecting to its listener shows they can, and five searches answered only over connections
//! the daemon opened show they cannot.

mod support;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use api_client::Client;
use api_types::search::SearchRequest;
use api_types::state::{Reachable, State};
use soulseek_proto::init::{PeerInit, PierceFirewall};
use soulseek_proto::peer::SearchResponse;
use soulseek_proto::server::{ConnectToPeerResponse, FileSearchRequest};
use soulseek_proto::{InitMessage, PeerConnectionType, PeerMessage, ServerRequest, ServerResponse};
use support::client::peer::{FakePeer, PeerSide};
use support::{scripted, until};
use uuid::Uuid;

fn reachable(state: &State) -> Reachable {
  state.server.listen.reachable
}

#[tokio::test]
async fn a_peer_reaching_the_listener_by_itself_shows_we_can_be_reached() {
  let dir = tempfile::tempdir().unwrap();
  let (_daemon, api, _server) = scripted(dir.path()).await;
  let state = api.application().await.unwrap();
  assert_eq!(reachable(&state), Reachable::Unknown);
  let listener = SocketAddr::from((Ipv4Addr::LOCALHOST, state.server.listen.port));
  let mut bob = PeerSide::connect(listener).await;
  let hello = InitMessage::PeerInit(PeerInit {
    username: "bob".into(),
    connection_type: PeerConnectionType::Peer,
    token: 0,
  });
  bob.send(&hello.encode()).await;
  until(&api, "reachable", |s| reachable(s) == Reachable::Yes).await;
}

/// Starts a search on `api` and has bob answer it over a connection the daemon opens to him at
/// the server's request; his answer ends the search, so the next one does not wait its turn.
async fn answered_over_our_connection(
  api: &Client,
  server: &mut support::client::ServerSide,
  bob: &FakePeer,
  relayed: u32,
) {
  let id = Uuid::new_v4();
  let request: SearchRequest = serde_json::from_value(serde_json::json!({
    "id": id,
    "searchText": "animals",
    "searchTimeout": 60_000,
    "responseLimit": 1,
    "filterResponses": false,
  }))
  .unwrap();
  api.start_search(&request).await.unwrap();
  let token = server
    .request_matching(|request| match request {
      ServerRequest::FileSearch(FileSearchRequest { token, .. }) => Some(token),
      _ => None,
    })
    .await;
  server
    .send(&ServerResponse::ConnectToPeer(ConnectToPeerResponse {
      username: "bob".into(),
      connection_type: PeerConnectionType::Peer,
      ip: Ipv4Addr::LOCALHOST,
      port: bob.port(),
      token: relayed,
      privileged: false,
      obfuscation: None,
    }))
    .await;
  let mut side = bob.accept().await;
  assert_eq!(
    side.read_init().await,
    InitMessage::PierceFirewall(PierceFirewall { token: relayed })
  );
  let answer = PeerMessage::SearchResponse(SearchResponse {
    username: "bob".into(),
    token,
    files: Vec::new(),
    has_free_upload_slot: true,
    upload_speed: 0,
    queue_length: 0,
    locked_files: Vec::new(),
  });
  side.send(&answer.encode()).await;
  // the daemon took the answer once the search counts it
  let started = std::time::Instant::now();
  while api.search(id, false).await.unwrap().response_count == 0 {
    assert!(started.elapsed() < support::PATIENCE, "never answered");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

#[tokio::test]
async fn five_searches_answered_only_over_our_own_connections_show_we_cannot_be_reached() {
  let dir = tempfile::tempdir().unwrap();
  let (_daemon, api, mut server) = scripted(dir.path()).await;
  let bob = FakePeer::start().await;
  for relayed in 1..5 {
    answered_over_our_connection(&api, &mut server, &bob, relayed).await;
  }
  let state = api.application().await.unwrap();
  assert_eq!(reachable(&state), Reachable::Unknown, "four are not enough");
  answered_over_our_connection(&api, &mut server, &bob, 5).await;
  until(&api, "unreachable", |s| reachable(s) == Reachable::No).await;
}
