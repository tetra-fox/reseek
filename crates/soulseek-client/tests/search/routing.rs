//! Searches routed through soulfind between two clients, the responder's answer travelling back
//! over a message connection to the searcher: an empty answer is never sent, an encoded one is
//! sent as it is, one confined to an address goes only to a connection from it, and a search
//! sent straight over a message connection is answered on it.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use crate::support::soulfind::{self, Soulfind, User};
use crate::support::{PATIENCE, free_port};
use soulseek_client::{
  ClientOptions, Event, SearchOptions, SearchReply, SearchResponseResolver, SearchScope,
};
use soulseek_proto::peer::{File, SearchRequest, SearchResponse};
use soulseek_proto::{PeerMessage, SearchQuery};
use tokio_util::sync::CancellationToken;

/// what the responder's resolver was asked, in order
type Asked = Arc<Mutex<Vec<(String, u32, String)>>>;

fn response(username: &str, token: u32, files: usize) -> SearchResponse {
  SearchResponse {
    username: username.into(),
    token,
    files: (0..files)
      .map(|n| File {
        code: 1,
        filename: format!("@@music\\match {n}.flac").as_str().into(),
        size: 1000,
        extension: "flac".into(),
        attributes: Vec::new(),
      })
      .collect(),
    has_free_upload_slot: true,
    upload_speed: 1000,
    queue_length: 0,
    locked_files: Vec::new(),
  }
}

/// How the responder's answer is made
#[derive(Clone)]
enum Made {
  /// the files, which the client encodes
  Plain,
  /// a frame encoded ahead
  Encoded,
  /// the files, for the searcher at the address held, read when the answer is made
  ConfinedTo(Arc<Mutex<IpAddr>>),
}

/// a resolver that answers as `username` with `files` files, made as `made`, and records what
/// it was asked
fn resolver(username: String, files: usize, asked: Asked, made: Made) -> SearchResponseResolver {
  Arc::new(move |searcher: String, token: u32, query: SearchQuery| {
    asked
      .lock()
      .unwrap()
      .push((searcher, token, query.search_text()));
    let response = response(&username, token, files);
    let made = made.clone();
    Box::pin(async move {
      Ok(Some(match made {
        Made::Plain => SearchReply::Response(response),
        Made::Encoded => SearchReply::Encoded(PeerMessage::SearchResponse(response).encode()),
        Made::ConfinedTo(to) => SearchReply::Confined {
          reply: Box::new(SearchReply::Response(response)),
          to: *to.lock().unwrap(),
        },
      }))
    })
  })
}

/// a responder that answers every search with `files` files made as `made`, and a searcher,
/// both listening
async fn responder_and_searcher(
  server: &Arc<Soulfind>,
  files: usize,
  made: Made,
) -> (User, User, Asked) {
  let asked = Asked::default();
  let username = soulfind::username("responder");
  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  options.resolvers.search_response_resolver =
    Some(resolver(username.clone(), files, asked.clone(), made));
  let responder = server.login_as(username, options).await;
  let searcher = server
    .login(
      "searcher",
      ClientOptions {
        listen_port: free_port(),
        ..ClientOptions::default()
      },
    )
    .await;
  (responder, searcher, asked)
}

/// waits until `responder` has delivered its answer to `token`
async fn delivered(responder: &mut User, token: u32) -> Arc<SearchReply> {
  responder
    .event(|event| match event {
      Event::SearchResponseDelivered(delivery) if delivery.token == token => {
        Some(delivery.search_response)
      }
      _ => None,
    })
    .await
}

#[tokio::test]
async fn an_encoded_answer_is_sent_as_it_is() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (responder, searcher, _asked) = responder_and_searcher(&server, 1, Made::Encoded).await;
  // the search ends with the one answer it waits for
  let options = SearchOptions {
    response_limit: 1,
    filter_responses: false,
    ..SearchOptions::default()
  };
  let cancel = CancellationToken::new();
  let searched = searcher.client.search(
    "encoded",
    SearchScope::User(vec![responder.username.clone()]),
    Some(21),
    options,
    &cancel,
  );
  let (_, responses) = tokio::time::timeout(PATIENCE, searched)
    .await
    .expect("the search ends")
    .unwrap();
  assert_eq!(responses, vec![response(&responder.username, 21, 1)]);
}

#[tokio::test]
async fn an_empty_answer_is_not_sent() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (mut responder, searcher, asked) = responder_and_searcher(&server, 0, Made::Plain).await;
  searcher
    .client
    .send_search_request(
      &SearchScope::User(vec![responder.username.clone()]),
      31,
      "x",
    )
    .await
    .unwrap();
  responder
    .event(|event| matches!(event, Event::SearchRequestReceived { token: 31, .. }).then_some(()))
    .await;
  // a later search proves the first was handled without a delivery
  searcher
    .client
    .send_search_request(
      &SearchScope::User(vec![responder.username.clone()]),
      32,
      "y",
    )
    .await
    .unwrap();
  let delivered = responder
    .event(|event| match event {
      Event::SearchResponseDelivered(delivery) => Some(delivery.token),
      Event::SearchRequestReceived { token: 32, .. } => Some(32),
      _ => None,
    })
    .await;
  assert_eq!(delivered, 32, "nothing was delivered for the empty answer");
  assert_eq!(asked.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_search_sent_over_a_message_connection_is_answered_on_it() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (mut responder, searcher, asked) = responder_and_searcher(&server, 1, Made::Plain).await;
  let conn = searcher
    .client
    .get_or_add_message_connection(&responder.username, None, &CancellationToken::new())
    .await
    .unwrap();
  conn
    .write(
      PeerMessage::SearchRequest(SearchRequest {
        token: 41,
        query: "direct".into(),
      })
      .encode(),
    )
    .await
    .unwrap();
  delivered(&mut responder, 41).await;
  assert_eq!(
    asked.lock().unwrap().as_slice(),
    [(searcher.username.clone(), 41, "direct".to_owned())]
  );
}

#[tokio::test]
async fn a_confined_answer_goes_only_to_a_connection_from_its_address() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let elsewhere = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
  for (confined, token, reaches) in [(true, 51, false), (false, 52, true)] {
    // the searcher connects from the address the server gives for it
    let searcher_at = Arc::new(Mutex::new(elsewhere));
    let made = Made::ConfinedTo(searcher_at.clone());
    let (mut responder, searcher, _asked) = responder_and_searcher(&server, 1, made).await;
    if !confined {
      let at = responder
        .client
        .get_user_endpoint(&searcher.username)
        .await
        .unwrap();
      *searcher_at.lock().unwrap() = at.ip();
    }
    let to = *searcher_at.lock().unwrap();
    let conn = searcher
      .client
      .get_or_add_message_connection(&responder.username, None, &CancellationToken::new())
      .await
      .unwrap();
    let request = PeerMessage::SearchRequest(SearchRequest {
      token,
      query: "direct".into(),
    });
    conn.write(request.encode()).await.unwrap();
    let delivered = responder
      .event(|event| match event {
        Event::SearchResponseDelivered(d) if d.token == token => Some(true),
        Event::SearchResponseDeliveryFailed(d) if d.token == token => Some(false),
        _ => None,
      })
      .await;
    assert_eq!(delivered, reaches, "confined to {to}");
  }
}
