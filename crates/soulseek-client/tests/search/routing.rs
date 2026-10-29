//! Searches routed through soulfind between two clients, the responder's answer travelling back
//! over a message connection to the searcher: an empty answer is never sent, an encoded one is
//! sent as it is, and a search sent straight over a message connection is answered on it.

use std::sync::{Arc, Mutex};

use crate::support::soulfind::{self, Soulfind, User};
use crate::support::{Logs, free_port};
use soulseek_client::{ClientOptions, Event, SearchReply, SearchResponseResolver, SearchScope};
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

/// a resolver that answers as `username` with `files` files and records what it was asked
fn resolver(username: String, files: usize, asked: Asked, encoded: bool) -> SearchResponseResolver {
  Arc::new(move |searcher: String, token: u32, query: SearchQuery| {
    asked
      .lock()
      .unwrap()
      .push((searcher, token, query.search_text()));
    let response = response(&username, token, files);
    Box::pin(async move {
      Ok(Some(if encoded {
        SearchReply::Encoded(PeerMessage::SearchResponse(response).encode())
      } else {
        SearchReply::Response(response)
      }))
    })
  })
}

/// a responder that answers every search with `files` files, and a searcher, both listening
async fn responder_and_searcher(
  server: &Arc<Soulfind>,
  files: usize,
  encoded: bool,
) -> (User, User, Asked) {
  let asked = Asked::default();
  let username = soulfind::username("responder");
  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  options.resolvers.search_response_resolver =
    Some(resolver(username.clone(), files, asked.clone(), encoded));
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

/// waits until the searcher's peer dispatcher has seen the answer to `token` from `responder`
async fn arrived(logs: &Logs, responder: &str, token: u32) {
  logs
    .wait_for(&[
      "search response for no active search",
      &format!("username=\"{responder}\""),
      &format!("token={token}"),
    ])
    .await;
}

#[tokio::test]
async fn an_encoded_answer_is_sent_as_it_is() {
  let logs = Logs::capture();
  let Some(server) = soulfind::server() else {
    return;
  };
  let (mut responder, searcher, _asked) = responder_and_searcher(&server, 1, true).await;
  searcher
    .client
    .send_search_request(
      &SearchScope::User(vec![responder.username.clone()]),
      21,
      "x",
    )
    .await
    .unwrap();
  let reply = delivered(&mut responder, 21).await;
  assert!(matches!(*reply, SearchReply::Encoded(_)));
  arrived(&logs, &responder.username, 21).await;
}

#[tokio::test]
async fn an_empty_answer_is_not_sent() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (mut responder, searcher, asked) = responder_and_searcher(&server, 0, false).await;
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
  let (mut responder, searcher, asked) = responder_and_searcher(&server, 1, false).await;
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
