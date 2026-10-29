//! Delivering search answers when the searcher is hard to reach, against a scripted server and
//! searcher: an answer that could not be delivered is kept, and goes out when the searcher acts
//! on our connection request late; one the searcher says it cannot fetch is given up.

use std::sync::Arc;
use std::time::Duration;

use crate::support::peer::PeerSide;
use crate::support::{Logs, Session, free_port, listening_options};
use soulseek_client::{ClientOptions, Event, SearchReply};
use soulseek_proto::init::PierceFirewall;
use soulseek_proto::peer::{File, SearchResponse};
use soulseek_proto::server::{
  CannotConnectResponse, ConnectToPeerRequest, FileSearchResponse, GetPeerAddressRequest,
  GetPeerAddressResponse,
};
use soulseek_proto::{InitMessage, PeerMessage, ServerRequest, ServerResponse};

fn answer(token: u32) -> SearchResponse {
  SearchResponse {
    username: crate::support::USERNAME.into(),
    token,
    files: vec![File {
      code: 1,
      filename: "@@music\\hit.mp3".into(),
      size: 3_000_000,
      extension: "mp3".into(),
      attributes: Vec::new(),
    }],
    has_free_upload_slot: true,
    upload_speed: 100_000,
    queue_length: 0,
    locked_files: Vec::new(),
  }
}

/// a responder that answers every search and gives up on a peer after `connect_timeout`
async fn responder(connect_timeout: Duration) -> Session {
  let mut options = listening_options();
  options.peer_connection_options.connect_timeout = connect_timeout;
  options.resolvers.search_response_resolver = Some(Arc::new(move |_, token, _| {
    Box::pin(async move { Ok(Some(SearchReply::Response(answer(token)))) })
  }));
  Session::with_options(ClientOptions { ..options }).await
}

/// Carol searches; the scripted server gives the responder an address nobody listens on and
/// returns the token of the connection request the responder has the server relay to her
async fn carol_searches(session: &mut Session, token: u32) -> u32 {
  session
    .server
    .send(&ServerResponse::FileSearch(FileSearchResponse {
      username: "carol".into(),
      token,
      query: "hit".into(),
    }))
    .await;
  session
    .server
    .request_matching(|request| match request {
      ServerRequest::GetPeerAddress(GetPeerAddressRequest { username }) if username == "carol" => {
        Some(())
      }
      _ => None,
    })
    .await;
  session
    .server
    .send(&ServerResponse::GetPeerAddress(GetPeerAddressResponse {
      username: "carol".into(),
      ip: std::net::Ipv4Addr::LOCALHOST,
      port: u32::from(free_port()),
      obfuscation: None,
    }))
    .await;
  session
    .server
    .request_matching(|request| match request {
      ServerRequest::ConnectToPeer(ConnectToPeerRequest {
        token, username, ..
      }) if username == "carol" => Some(token),
      _ => None,
    })
    .await
}

/// carol connects to the responder, late, answering the request `token`
async fn carol_pierces(session: &Session, token: u32) -> PeerSide {
  let mut carol = PeerSide::connect(session.listen_address()).await;
  carol
    .send(&InitMessage::PierceFirewall(PierceFirewall { token }).encode())
    .await;
  carol
}

#[tokio::test]
async fn a_kept_answer_reaches_a_searcher_who_connects_late() {
  let logs = Logs::capture();
  let mut session = responder(Duration::from_millis(300)).await;
  let request = carol_searches(&mut session, 5).await;
  logs
    .wait_for(&["cached response for potential delayed delivery"])
    .await;
  let mut carol = carol_pierces(&session, request).await;
  assert_eq!(
    carol.read_peer().await,
    PeerMessage::SearchResponse(answer(5))
  );
  let delivered = session
    .event(|event| match event {
      Event::SearchResponseDelivered(delivery) => Some(delivery),
      _ => None,
    })
    .await;
  assert_eq!((delivered.username.as_str(), delivered.token), ("carol", 5));
  // the late connection is now carol's message connection
  assert_eq!(session.client.message_connections()[0].0, "carol");
}

#[tokio::test]
async fn a_kept_answer_is_given_up_when_the_searcher_cannot_connect() {
  let logs = Logs::capture();
  let mut session = responder(Duration::from_millis(300)).await;
  let request = carol_searches(&mut session, 6).await;
  logs
    .wait_for(&["cached response for potential delayed delivery"])
    .await;
  session
    .server
    .send(&ServerResponse::CannotConnect(CannotConnectResponse {
      token: request,
      username: None,
    }))
    .await;
  let failed = session
    .event(|event| match event {
      Event::SearchResponseDeliveryFailed(delivery) => Some(delivery),
      _ => None,
    })
    .await;
  assert_eq!((failed.username.as_str(), failed.token), ("carol", 6));
  // nothing is kept under that request any more
  let mut carol = carol_pierces(&session, request).await;
  carol.closed().await;
}

#[tokio::test]
async fn a_searcher_refusing_while_we_still_try_gets_nothing_kept() {
  let mut session = responder(Duration::from_secs(5)).await;
  let started = tokio::time::Instant::now();
  let request = carol_searches(&mut session, 7).await;
  session
    .server
    .send(&ServerResponse::CannotConnect(CannotConnectResponse {
      token: request,
      username: None,
    }))
    .await;
  let failed = session
    .event(|event| match event {
      Event::SearchResponseDeliveryFailed(delivery) => Some(delivery.token),
      _ => None,
    })
    .await;
  assert_eq!(failed, 7);
  // the refusal ended the wait for carol's connection long before its timeout
  assert!(started.elapsed() < Duration::from_secs(4));
  let mut carol = carol_pierces(&session, request).await;
  carol.closed().await;
}
