//! Connection requests relayed by a scripted server: we answer a peer's request by dialling it,
//! report the ones we cannot honour so the peer stops waiting, retry when a peer asks again as
//! older clients do, and give up our own request at once when the peer reports it cannot
//! connect.

mod support;

use std::net::Ipv4Addr;
use std::time::Duration;

use soulseek_client::ClientError;
use soulseek_proto::init::PierceFirewall;
use soulseek_proto::server::{
  CannotConnectRequest, CannotConnectResponse, ConnectToPeerRequest, ConnectToPeerResponse,
};
use soulseek_proto::{InitMessage, PeerConnectionType, PeerMessage, ServerRequest, ServerResponse};
use support::peer::FakePeer;
use support::{Session, free_port, listening_options, options};
use tokio_util::sync::CancellationToken;

fn request(
  username: &str,
  connection_type: PeerConnectionType,
  port: u32,
  token: u32,
) -> ServerResponse {
  ServerResponse::ConnectToPeer(ConnectToPeerResponse {
    username: username.into(),
    connection_type,
    ip: Ipv4Addr::LOCALHOST,
    port,
    token,
    privileged: false,
    obfuscation: None,
  })
}

async fn cannot_connect(session: &mut Session) -> CannotConnectRequest {
  session
    .server
    .request_matching(|request| match request {
      ServerRequest::CannotConnect(report) => Some(report),
      _ => None,
    })
    .await
}

#[tokio::test]
async fn a_peer_we_cannot_dial_back_is_told_so() {
  let mut session = Session::start().await;
  let closed_port = u32::from(free_port());
  session
    .server
    .send(&request("erin", PeerConnectionType::Peer, closed_port, 31))
    .await;
  assert_eq!(
    cannot_connect(&mut session).await,
    CannotConnectRequest {
      token: 31,
      username: "erin".into()
    }
  );
}

#[tokio::test]
async fn transfer_and_child_requests_we_do_not_take_are_refused_at_once() {
  let mut session = Session::start().await;
  let peer = FakePeer::start().await;
  // no download waits for a file from this user
  session
    .server
    .send(&request(
      "stranger",
      PeerConnectionType::Transfer,
      peer.port(),
      32,
    ))
    .await;
  assert_eq!(cannot_connect(&mut session).await.token, 32);
  // we have neither a parent nor a branch, so we take no children
  session
    .server
    .send(&request(
      "child",
      PeerConnectionType::Distributed,
      peer.port(),
      33,
    ))
    .await;
  assert_eq!(cannot_connect(&mut session).await.token, 33);
}

#[tokio::test]
async fn a_peer_asking_again_after_a_failure_is_dialled_again() {
  let mut session = Session::start().await;
  session
    .server
    .send(&request(
      "frank",
      PeerConnectionType::Peer,
      u32::from(free_port()),
      34,
    ))
    .await;
  assert_eq!(cannot_connect(&mut session).await.token, 34);
  // an older client retries its request, this time from where it listens
  let frank = FakePeer::start().await;
  session
    .server
    .send(&request(
      "frank",
      PeerConnectionType::Peer,
      frank.port(),
      35,
    ))
    .await;
  let mut connection = frank.accept().await;
  assert_eq!(
    connection.read_init().await,
    InitMessage::PierceFirewall(PierceFirewall { token: 35 })
  );
  connection.send(&PeerMessage::InfoRequest.encode()).await;
  assert!(matches!(
    connection.read_peer().await,
    PeerMessage::InfoResponse(_)
  ));
  assert_eq!(session.client.message_connections()[0].0, "frank");
}

#[tokio::test]
async fn a_request_from_a_peer_we_are_connected_to_is_ignored() {
  let mut session = Session::with_options(listening_options()).await;
  let mut grace = session.peer("grace").await;
  grace.send(&PeerMessage::InfoRequest.encode()).await;
  grace.read_peer().await;
  let elsewhere = FakePeer::start().await;
  session
    .server
    .send(&request(
      "grace",
      PeerConnectionType::Peer,
      elsewhere.port(),
      36,
    ))
    .await;
  // neither dialled nor refused
  assert!(
    tokio::time::timeout(Duration::from_millis(300), elsewhere.accept())
      .await
      .is_err()
  );
  session.settle().await;
}

#[tokio::test]
async fn our_request_ends_at_once_when_the_peer_cannot_connect() {
  let mut options = options();
  options.peer_connection_options.connect_timeout = Duration::from_secs(5);
  let mut session = Session::with_options(options).await;
  let client = session.client.clone();
  let unreachable = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, free_port()));
  let started = tokio::time::Instant::now();
  let connecting = tokio::spawn(async move {
    client
      .get_or_add_message_connection("dave", Some(unreachable), &CancellationToken::new())
      .await
  });
  let token = session
    .server
    .request_matching(|request| match request {
      ServerRequest::ConnectToPeer(ConnectToPeerRequest {
        token, username, ..
      }) if username == "dave" => Some(token),
      _ => None,
    })
    .await;
  session
    .server
    .send(&ServerResponse::CannotConnect(CannotConnectResponse {
      token,
      username: None,
    }))
    .await;
  let error = connecting.await.unwrap().unwrap_err();
  assert!(matches!(error, ClientError::Connection { .. }), "{error:?}");
  assert!(
    error
      .to_string()
      .starts_with("Failed to establish a direct or indirect message connection to dave"),
    "{error}"
  );
  assert!(started.elapsed() < Duration::from_secs(4));
}
