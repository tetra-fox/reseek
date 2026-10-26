//! What other users get from the daemon over soulfind: a search of its shares answered with the
//! files that match and its upload figures, its share listing, a folder's contents and its user
//! info.

mod support;

use std::time::Duration;

use soulseek_client::{BrowseOptions, ClientOptions, SearchOptions, SearchScope};
use support::soulfind;
use support::{PATIENCE, daemon, free_port, logged_in, signed_in, until};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn another_user_searches_browses_and_asks_the_daemon() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let album = dir.path().join("Music").join("Abbey Road");
  std::fs::create_dir_all(&album).unwrap();
  std::fs::write(album.join("01 - Come Together.mp3"), "one").unwrap();
  std::fs::write(album.join("02 - Something.mp3"), "two").unwrap();
  let name = soulfind::username("daemon");
  let shared = dir.path().join("Music");
  let daemon = daemon(
    dir.path(),
    &server,
    &name,
    &["--shared", shared.to_str().unwrap(), "--upload-slots", "3"],
  );
  let api = signed_in(&daemon).await;
  let state = until(&api, "logged in with its shares", |s| {
    logged_in(s) && s.shares.files == 2
  })
  .await;

  let other = server
    .login(
      "other",
      ClientOptions {
        listen_port: free_port(),
        ..ClientOptions::default()
      },
    )
    .await;
  let cancel = CancellationToken::new();
  let options = SearchOptions {
    search_timeout: Duration::from_millis(1500),
    ..SearchOptions::default()
  };
  let (search, responses) = tokio::time::timeout(
    PATIENCE,
    other.client.search(
      "abbey -something",
      SearchScope::User(vec![name.clone()]),
      None,
      options,
      &cancel,
    ),
  )
  .await
  .expect("the search ends")
  .unwrap();
  assert_eq!(responses.len(), 1, "{responses:?}");
  let response = &responses[0];
  assert_eq!(response.username, name);
  assert_eq!(response.token, search.token);
  let files: Vec<&str> = response.files.iter().map(|f| f.filename.as_str()).collect();
  assert_eq!(files, ["Music\\Abbey Road\\01 - Come Together.mp3"]);
  assert_eq!(response.files[0].size, 3);
  assert!(response.has_free_upload_slot);
  assert_eq!(response.queue_length, 0);
  assert_eq!(
    response.upload_speed, state.user.statistics.average_speed,
    "the speed the server keeps for us"
  );
  assert!(response.locked_files.is_empty());

  let listing = other
    .client
    .browse(&name, BrowseOptions::default(), &cancel)
    .await
    .unwrap();
  let directories: Vec<(&str, usize)> = listing
    .directories
    .iter()
    .map(|d| (d.name.as_str(), d.files.len()))
    .collect();
  assert_eq!(directories, [("Music", 0), ("Music\\Abbey Road", 2)]);

  let folder = other
    .client
    .get_directory_contents(&name, "Music\\Abbey Road", None, None, &cancel)
    .await
    .unwrap();
  assert_eq!(folder.len(), 1);
  let names: Vec<&str> = folder[0]
    .files
    .iter()
    .map(|f| f.filename.as_str())
    .collect();
  assert_eq!(names, ["01 - Come Together.mp3", "02 - Something.mp3"]);

  let info = other.client.get_user_info(&name, &cancel).await.unwrap();
  assert_eq!(info.description, "A reseek user.");
  assert_eq!(
    (
      info.upload_slots,
      info.queue_length,
      info.has_free_upload_slot
    ),
    (3, 0, true)
  );

  let metrics = api.metrics().await.unwrap();
  let value = |name: &str| metrics[name].samples.as_ref().unwrap()[0].value;
  assert_eq!(value("reseek_search_incoming_requests_received"), 1.0);
  assert_eq!(value("reseek_search_incoming_responses_sent"), 1.0);
  assert_eq!(value("reseek_browse_requests_received"), 1.0);
  assert_eq!(value("reseek_browse_responses_sent"), 1.0);
}
