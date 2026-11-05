//! Our searches between clients on soulfind: every scope, the states a search goes through, the
//! limits and filters on what it takes, the one way our own search reaches our own answers,
//! searches waiting their turn, tokens taken once, and a disconnect canceling them.

use std::sync::Arc;
use std::time::Duration;

use crate::support::soulfind::{self, Soulfind, User};
use crate::support::{PATIENCE, next_event};
use soulseek_client::{
  ClientError, ClientOptions, Event, Search, SearchOptions, SearchReply, SearchResponseResolver,
  SearchScope, SearchState,
};
use soulseek_proto::peer::{File, SearchResponse};
use soulseek_proto::{RoomType, SearchQuery};
use tokio_util::sync::CancellationToken;

/// a resolver that answers every search as `username` with `files` files
fn answering(username: String, files: usize) -> SearchResponseResolver {
  Arc::new(move |_, token: u32, _: SearchQuery| {
    let response = SearchResponse {
      username: username.clone(),
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
      upload_speed: 5000,
      queue_length: 0,
      locked_files: Vec::new(),
    };
    Box::pin(async move { Ok(Some(SearchReply::Response(response))) })
  })
}

/// a user that answers every search with `files` files
async fn responder(server: &Arc<Soulfind>, prefix: &str, files: usize) -> User {
  let username = soulfind::username(prefix);
  let mut options = soulfind::listening();
  options.resolvers.search_response_resolver = Some(answering(username.clone(), files));
  server.login_as(username, options).await
}

/// options for a search that ends soon after its last response
fn quick() -> SearchOptions {
  SearchOptions {
    search_timeout: Duration::from_millis(1500),
    ..SearchOptions::default()
  }
}

async fn search(
  searcher: &User,
  scope: SearchScope,
  options: SearchOptions,
) -> (Search, Vec<SearchResponse>) {
  tokio::time::timeout(
    PATIENCE,
    searcher.client.search(
      "flac match",
      scope,
      None,
      options,
      &CancellationToken::new(),
    ),
  )
  .await
  .expect("the search ends")
  .unwrap()
}

#[tokio::test]
async fn network_and_room_searches_take_the_answers_and_end_when_they_stop() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let responder = responder(&server, "responder", 2).await;
  // the searcher answers too, but never its own search unless it asks itself
  let mut searcher = responder_like(&server).await;
  let room = soulfind::username("searched");
  for user in [&responder, &searcher] {
    user
      .client
      .join_room(&room, RoomType::Public)
      .await
      .unwrap();
  }
  let scopes = [SearchScope::Network, SearchScope::Rooms(vec![room.clone()])];
  for scope in scopes {
    let mut events = searcher.client.subscribe();
    let (done, responses) = search(&searcher, scope.clone(), quick()).await;
    assert_eq!(
      done.state,
      SearchState::COMPLETED | SearchState::TIMED_OUT,
      "{scope:?}"
    );
    let from: Vec<_> = responses
      .iter()
      .map(|response| response.username.as_str())
      .collect();
    assert_eq!(from, [responder.username.as_str()], "{scope:?}");
    assert_eq!((done.response_count, done.file_count), (1, 2));
    assert_eq!(done.query.search_text(), "flac match");
    let states = search_states(&mut events).await;
    assert_eq!(
      states,
      [
        SearchState::REQUESTED,
        SearchState::QUEUED,
        SearchState::IN_PROGRESS,
        SearchState::COMPLETED | SearchState::TIMED_OUT,
      ]
    );
  }
  // asking ourselves on purpose reaches our own answers
  let (done, responses) = search(
    &searcher,
    SearchScope::User(vec![searcher.username.clone()]),
    quick(),
  )
  .await;
  assert_eq!(done.response_count, 1);
  assert_eq!(responses[0].username, searcher.username);
  searcher
    .event(|event| matches!(event, Event::SearchResponseDelivered(_)).then_some(()))
    .await;
}

/// a searcher that answers every search with one file of its own
async fn responder_like(server: &Arc<Soulfind>) -> User {
  responder(server, "searcher", 1).await
}

/// the states of the next search, until it completed
async fn search_states(events: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<SearchState> {
  let mut states = Vec::new();
  loop {
    let state = next_event(events, |event| match event {
      Event::SearchStateChanged { search, .. } => Some(search.state),
      _ => None,
    })
    .await;
    states.push(state);
    if state.contains(SearchState::COMPLETED) {
      return states;
    }
  }
}

#[tokio::test]
async fn a_wishlist_search_goes_out_like_a_network_search() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let responder = responder(&server, "responder", 1).await;
  let searcher = server.login("searcher", soulfind::listening()).await;
  let (done, responses) = search(&searcher, SearchScope::Wishlist, quick()).await;
  assert_eq!(done.state, SearchState::COMPLETED | SearchState::TIMED_OUT);
  assert_eq!(responses[0].username, responder.username);
}

#[tokio::test]
async fn a_search_ends_once_it_has_enough_responses_or_files() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let _first = responder(&server, "responder", 5).await;
  let _second = responder(&server, "responder", 5).await;
  let searcher = server.login("searcher", soulfind::listening()).await;
  let options = SearchOptions {
    response_limit: 1,
    search_timeout: Duration::from_secs(30),
    ..SearchOptions::default()
  };
  let (done, responses) = search(&searcher, SearchScope::Network, options).await;
  assert_eq!(
    done.state,
    SearchState::COMPLETED | SearchState::RESPONSE_LIMIT_REACHED
  );
  assert_eq!(responses.len(), 1);
  let options = SearchOptions {
    file_limit: 3,
    search_timeout: Duration::from_secs(30),
    ..SearchOptions::default()
  };
  let (done, _) = search(&searcher, SearchScope::Network, options).await;
  assert_eq!(
    done.state,
    SearchState::COMPLETED | SearchState::FILE_LIMIT_REACHED
  );
  assert!(done.file_count >= 3);
}

#[tokio::test]
async fn filtered_out_responses_are_not_taken() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let _responder = responder(&server, "responder", 2).await;
  let searcher = server.login("searcher", soulfind::listening()).await;
  let mut options = quick();
  options.minimum_peer_upload_speed = 1_000_000;
  let (done, responses) = search(&searcher, SearchScope::Network, options).await;
  assert!(responses.is_empty());
  assert_eq!(done.response_count, 0);
}

#[tokio::test]
async fn searches_past_the_bound_wait_their_turn_and_a_token_is_taken_once() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let searcher = server
    .login(
      "searcher",
      ClientOptions {
        maximum_concurrent_searches: 1,
        ..soulfind::listening()
      },
    )
    .await;
  let mut events = searcher.client.subscribe();
  let first = tokio::spawn({
    let client = searcher.client.clone();
    async move {
      client
        .search(
          "first one",
          SearchScope::Network,
          Some(7),
          quick(),
          &CancellationToken::new(),
        )
        .await
    }
  });
  next_event(&mut events, |event| match event {
    Event::SearchStateChanged { search, .. } if search.state == SearchState::IN_PROGRESS => {
      Some(())
    }
    _ => None,
  })
  .await;
  let taken = searcher
    .client
    .search(
      "again",
      SearchScope::Network,
      Some(7),
      quick(),
      &CancellationToken::new(),
    )
    .await;
  assert!(matches!(taken, Err(ClientError::DuplicateToken(7))));
  let second = tokio::spawn({
    let client = searcher.client.clone();
    async move {
      client
        .search(
          "second one",
          SearchScope::Network,
          None,
          quick(),
          &CancellationToken::new(),
        )
        .await
    }
  });
  // the second stays queued until the first is done
  let in_progress = next_event(&mut events, |event| match event {
    Event::SearchStateChanged { search, .. }
      if search.state == SearchState::IN_PROGRESS
        || search.state.contains(SearchState::COMPLETED) =>
    {
      Some(search.token)
    }
    _ => None,
  })
  .await;
  assert_eq!(
    in_progress, 7,
    "the first completes before the second starts"
  );
  first.await.unwrap().unwrap();
  second.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_disconnect_cancels_every_search() {
  let Some(server) = soulfind::server() else {
    return;
  };
  // the second search waits for the first's turn
  let searcher = server
    .login(
      "searcher",
      ClientOptions {
        maximum_concurrent_searches: 1,
        ..soulfind::listening()
      },
    )
    .await;
  let mut events = searcher.client.subscribe();
  let start = |token: u32| {
    let client = searcher.client.clone();
    tokio::spawn(async move {
      let options = SearchOptions {
        search_timeout: Duration::from_secs(60),
        ..SearchOptions::default()
      };
      client
        .search(
          "long wait",
          SearchScope::Network,
          Some(token),
          options,
          &CancellationToken::new(),
        )
        .await
    })
  };
  let running = start(1);
  next_event(&mut events, |event| match event {
    Event::SearchStateChanged { search, .. } if search.state == SearchState::IN_PROGRESS => {
      Some(())
    }
    _ => None,
  })
  .await;
  let queued = start(2);
  next_event(&mut events, |event| match event {
    Event::SearchStateChanged { search, .. }
      if search.token == 2 && search.state == SearchState::QUEUED =>
    {
      Some(())
    }
    _ => None,
  })
  .await;
  searcher.client.disconnect(None, None).await;
  for search in [running, queued] {
    let result = tokio::time::timeout(PATIENCE, search)
      .await
      .unwrap()
      .unwrap();
    assert!(matches!(result, Err(ClientError::Canceled)), "{result:?}");
  }
  let mut ended = Vec::new();
  while ended.len() < 2 {
    let (token, state) = next_event(&mut events, |event| match event {
      Event::SearchStateChanged { search, .. } => Some((search.token, search.state)),
      _ => None,
    })
    .await;
    assert_ne!(
      (token, state),
      (2, SearchState::IN_PROGRESS),
      "the queued search is never sent"
    );
    if state.contains(SearchState::COMPLETED) {
      ended.push((token, state));
    }
  }
  ended.sort_unstable_by_key(|(token, _)| *token);
  let canceled = SearchState::COMPLETED | SearchState::CANCELLED;
  assert_eq!(ended, [(1, canceled), (2, canceled)]);
}
