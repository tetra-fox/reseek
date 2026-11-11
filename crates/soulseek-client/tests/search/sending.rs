//! A search whose request is still being written to a scripted server: an answer that comes
//! back meanwhile is taken, as the server can pass the request on before the write is seen to
//! end.

use std::time::Duration;

use crate::support::{FakeServer, Session, listening_options};
use soulseek_client::{Event, SearchOptions, SearchOutcome, SearchScope, SearchState};
use soulseek_proto::PeerMessage;
use soulseek_proto::peer::SearchResponse;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn an_answer_that_comes_while_the_request_is_written_is_taken() {
  let mut options = listening_options();
  options.server_connection_options.write_queue_size = 4096;
  let mut session = Session::on(FakeServer::with_receive_buffer(4096).await, options).await;
  // the server stops reading, so every later write to it waits, the search's too
  session.stall_server_writes();
  const TOKEN: u32 = 7;
  let searching = {
    let client = session.client.clone();
    tokio::spawn(async move {
      let options = SearchOptions {
        response_limit: 1,
        filter_responses: false,
        search_timeout: Duration::from_secs(600),
        ..SearchOptions::default()
      };
      let scope = SearchScope::Network;
      let cancel = CancellationToken::new();
      client
        .search("animals", scope, Some(TOKEN), options, &cancel)
        .await
    })
  };
  session
    .event(|event| match event {
      Event::SearchStateChanged { search, .. } if search.state == SearchState::InProgress => {
        Some(())
      }
      _ => None,
    })
    .await;
  let mut bob = session.peer("bob").await;
  let answer = PeerMessage::SearchResponse(SearchResponse {
    username: "bob".into(),
    token: TOKEN,
    files: Vec::new(),
    has_free_upload_slot: true,
    upload_speed: 0,
    queue_length: 0,
    locked_files: Vec::new(),
  });
  bob.send(&answer.encode()).await;
  // taken while the write still waits
  let taken = session
    .event(|event| match event {
      Event::SearchResponseReceived { search, .. } => Some(search.response_count),
      _ => None,
    })
    .await;
  assert_eq!(taken, 1);
  // the limit ends the search at once, its request still waiting to be written
  let (search, responses) = searching.await.unwrap().unwrap();
  assert_eq!(
    search.state,
    SearchState::Completed(SearchOutcome::ResponseLimitReached)
  );
  assert_eq!(responses.len(), 1);
}
