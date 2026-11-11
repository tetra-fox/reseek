//! Searches the daemon starts over soulfind, another user's client answering them: the
//! responses are kept with their counts, a locked file is marked, a file too small for what it
//! claims carries the hint, and a search ends on its response limit.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use api_client::Client;
use api_types::search::{Search, SearchOutcome, SearchRequest, SearchState, SuspectReason};
use soulseek_client::{ClientOptions, SearchReply};
use soulseek_proto::FileAttributeType;
use soulseek_proto::peer::{File, FileAttribute, SearchResponse};
use support::soulfind::{self, Soulfind, User};
use support::{PATIENCE, daemon, free_port, logged_in, signed_in, until};
use uuid::Uuid;

/// A file of `size` bytes and `seconds` long at `kbps`.
fn file(name: &str, size: u64, seconds: u32, kbps: u32) -> File {
  File {
    code: 1,
    filename: name.into(),
    size,
    extension: String::new(),
    attributes: vec![
      FileAttribute {
        attribute_type: FileAttributeType::BitRate,
        value: kbps,
      },
      FileAttribute {
        attribute_type: FileAttributeType::Duration,
        value: seconds,
      },
    ],
  }
}

/// A user whose client answers every search with two files, one of them a "320 kbps" MP3 only
/// big enough for 128, and one locked file.
async fn responder(server: &Arc<Soulfind>) -> User {
  let username = soulfind::username("responder");
  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  let answering = username.clone();
  options.resolvers.search_response_resolver = Some(Arc::new(move |_, token, _| {
    let response = SearchResponse {
      username: answering.clone(),
      token,
      files: vec![
        file("@@music\\Animals\\01 Dogs.mp3", 3_840_000, 240, 320),
        file("@@music\\Animals\\02 Pigs.mp3", 9_600_000, 240, 320),
      ],
      has_free_upload_slot: true,
      upload_speed: 5000,
      queue_length: 0,
      locked_files: vec![file("@@music\\Animals\\03 Sheep.mp3", 9_600_000, 240, 320)],
    };
    Box::pin(async move { Ok(Some(SearchReply::Response(response))) })
  }));
  server.login_as(username, options).await
}

/// The search `id` once it completed.
async fn completed(api: &Client, id: Uuid) -> Search {
  let started = Instant::now();
  loop {
    let search = api.search(id, false).await.unwrap();
    if search.state.is_complete() {
      return search;
    }
    assert!(started.elapsed() < PATIENCE, "never completed: {search:?}");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

/// A search for `text` that ends on its first response.
fn request(text: &str) -> SearchRequest {
  serde_json::from_value(serde_json::json!({
    "searchText": text,
    "responseLimit": 1,
    "searchTimeout": 15_000,
  }))
  .unwrap()
}

#[tokio::test]
async fn a_search_keeps_a_peers_files_and_ends_on_its_limit() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "logged in", logged_in).await;
  let peer = responder(&server).await;

  let limited = api
    .start_search(&request("pink  floyd -live"))
    .await
    .unwrap();
  assert_eq!(limited.search_text, "pink floyd -live");
  let limited = completed(&api, limited.id).await;
  assert_eq!(
    limited.state,
    SearchState::Completed(SearchOutcome::ResponseLimitReached)
  );
  assert_eq!(
    (
      limited.response_count,
      limited.file_count,
      limited.locked_file_count
    ),
    (1, 2, 1)
  );
  assert!(limited.response_bytes > 0);
  let responses = api.search_responses(limited.id).await.unwrap();
  assert_eq!(responses.len(), 1);
  let response = &responses[0];
  assert_eq!(response.username, peer.username);
  assert_eq!((response.file_count, response.locked_file_count), (2, 1));
  let dogs = &response.files[0];
  let hint = dogs.suspect.expect("a 320 kbps file weighing 128");
  assert_eq!(hint.reason, SuspectReason::BelowDeclaredBitrate);
  assert_eq!((hint.implied_kbps, hint.expected_kbps), (128, 320));
  assert_eq!(response.files[1].suspect, None);
  assert!(!dogs.is_locked);
  assert!(response.locked_files[0].is_locked);
}
