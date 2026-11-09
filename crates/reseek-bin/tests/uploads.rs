//! Uploads by the daemon over soulfind to other users' clients: with one slot, files are queued
//! and sent in turn, a second request waits its place, the forecast says so in search responses
//! and the user info, why it waits is told, a canceled upload leaves the listings once it
//! ended, and a live change of the group's speed lets the running upload finish and the queued
//! one start; an upload that fails once started is queued anew when its peer, told of the
//! failure, asks again at once.

mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use api_client::Client;
use api_types::options::OptionsPatch;
use api_types::transfers::{Transfer, TransferStates, UploadsQuery};
use serde_json::json;
use soulseek_client::{
  ClientError, ClientOptions, DownloadOutput, DownloadRequest, Event, SearchOptions, SearchScope,
};
use support::soulfind::{self, Soulfind, User};
use support::transfers::{content, file, scratch};
use support::{PATIENCE, daemon, events, free_port, logged_in, signed_in, until};
use tokio_util::sync::CancellationToken;

/// A user of the test's, taking the daemon's transfer connections.
async fn peer(server: &Arc<Soulfind>, prefix: &str) -> User {
  let options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  server.login(prefix, options).await
}

/// The daemon in `dir` sharing `dir/share` with `config`, logged in once it shares `files`.
async fn sharing(
  dir: &Path,
  server: &Arc<Soulfind>,
  name: &str,
  config: &str,
  files: u32,
) -> (support::Daemon, Client) {
  std::fs::write(dir.join("config.toml"), config).unwrap();
  let shared = dir.join("share");
  let daemon = daemon(dir, server, name, &["--shared", shared.to_str().unwrap()]);
  let api = signed_in(&daemon).await;
  until(&api, "logged in with its shares", |s| {
    logged_in(s) && s.shares.files == files
  })
  .await;
  (daemon, api)
}

/// Starts downloading `filename` of `size` bytes from `from` into a file of its own.
fn download(
  peer: &User,
  from: &str,
  filename: &str,
  size: usize,
) -> (
  tokio::task::JoinHandle<Result<soulseek_client::Transfer, ClientError>>,
  std::path::PathBuf,
) {
  let into = scratch().join("downloaded");
  let request = DownloadRequest::new(
    from,
    filename,
    size as u64,
    DownloadOutput::File(into.clone()),
  );
  let client = peer.client.clone();
  let task = tokio::spawn(async move { client.download(request, &CancellationToken::new()).await });
  (task, into)
}

/// The latest upload of `filename` to `username`, removed ones too.
async fn upload_of(api: &Client, username: &str, filename: &str) -> Option<Transfer> {
  let query = UploadsQuery {
    include_removed: true,
    ..UploadsQuery::default()
  };
  api
    .uploads(&query)
    .await
    .unwrap()
    .into_iter()
    .filter(|user| user.username == username)
    .flat_map(|user| user.directories)
    .flat_map(|directory| directory.files)
    .filter(|transfer| transfer.filename == filename)
    .max_by_key(|transfer| transfer.requested_at)
}

/// The upload of `filename` to `username` once `ready` holds for it.
async fn upload_when(
  api: &Client,
  username: &str,
  filename: &str,
  what: &str,
  ready: impl Fn(&Transfer) -> bool,
) -> Transfer {
  let started = Instant::now();
  loop {
    let upload = upload_of(api, username, filename).await;
    if let Some(upload) = upload.as_ref().filter(|u| ready(u)) {
      return upload.clone();
    }
    assert!(started.elapsed() < PATIENCE * 2, "never {what}: {upload:?}");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

/// The value of the metric `name`.
async fn metric(api: &Client, name: &str) -> f64 {
  let metrics = api.metrics().await.unwrap();
  metrics[name].samples.as_ref().unwrap()[0].value
}

fn succeeded() -> TransferStates {
  TransferStates::Completed | TransferStates::Succeeded
}

#[tokio::test]
async fn a_second_request_waits_its_place_and_starts_once_the_first_ends() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let album = dir.path().join("share").join("album");
  std::fs::create_dir_all(&album).unwrap();
  let (big, small) = (100_000, 1_000);
  file(&album, "big.flac", big);
  file(&album, "small.flac", small);
  let name = soulfind::username("daemon");
  // the test's users share nothing, so they are leechers, held to 1 KiB/s while waiting in the
  // default queue, since the leechers set no queue of their own
  let config = "[transfers.upload]\nslots = 1\n\
                [transfers.groups.leechers.upload]\nspeed_limit = 1\n";
  let (_daemon, api) = sharing(dir.path(), &server, &name, config, 2).await;
  let (first, second, third) = (
    peer(&server, "first").await,
    peer(&server, "second").await,
    peer(&server, "third").await,
  );
  let (big_name, small_name) = ("share\\album\\big.flac", "share\\album\\small.flac");

  // 1 KiB/s holds the only slot
  let (running, _) = download(&first, &name, big_name, big);
  upload_when(&api, &first.username, big_name, "in progress", |t| {
    t.state == TransferStates::InProgress
  })
  .await;
  let (canceled, _) = download(&second, &name, small_name, small);
  let waiting = upload_when(&api, &second.username, small_name, "queued", |t| {
    t.state == TransferStates::Queued | TransferStates::Locally
  })
  .await;
  let cancel = CancellationToken::new();
  let place = second
    .client
    .get_download_place_in_queue(&name, small_name, &cancel)
    .await
    .unwrap();
  assert_eq!(
    place, 2,
    "behind the upload running in the group, counted from 1"
  );
  let why = api
    .upload_queue_status(&second.username, waiting.id)
    .await
    .unwrap();
  assert_eq!(
    (
      why.position,
      why.group.as_str(),
      why.group_slots_used,
      why.group_slots,
      why.global_slots_used,
      why.global_slots,
      why.user_active
    ),
    (Some(1), "default", 1, 1, 1, 1, 0)
  );

  // a newcomer is told where a request of theirs would land; asking for the user info finds
  // their group, which the search response then reads
  let info = third.client.get_user_info(&name, &cancel).await.unwrap();
  assert_eq!(
    (
      info.upload_slots,
      info.queue_length,
      info.has_free_upload_slot
    ),
    (1, 2, false),
    "two users in the group"
  );
  let options = SearchOptions {
    search_timeout: Duration::from_millis(1500),
    ..SearchOptions::default()
  };
  let (_, responses) = third
    .client
    .search(
      "small",
      SearchScope::User(vec![name.clone()]),
      None,
      options,
      &cancel,
    )
    .await
    .unwrap();
  assert_eq!(responses.len(), 1, "{responses:?}");
  assert!(!responses[0].has_free_upload_slot);
  assert_eq!(responses[0].queue_length, 2);

  // a canceled upload tells its peer and leaves the listings once it ended
  api
    .cancel_transfer(
      api_types::transfers::TransferDirection::Upload,
      &second.username,
      waiting.id,
      true,
    )
    .await
    .unwrap();
  let refused = tokio::time::timeout(PATIENCE, canceled)
    .await
    .expect("the download ends")
    .unwrap();
  assert!(
    matches!(&refused, Err(ClientError::TransferRejected(reason)) if reason == "Cancelled"),
    "{refused:?}"
  );
  let ended = upload_when(&api, &second.username, small_name, "removed", |t| t.removed).await;
  assert_eq!(
    ended.state,
    TransferStates::Completed | TransferStates::Cancelled
  );

  // asked again, it waits; a faster group lets the first finish and the second start
  let (again, into) = download(&second, &name, small_name, small);
  upload_when(&api, &second.username, small_name, "queued again", |t| {
    !t.removed && t.state == TransferStates::Queued | TransferStates::Locally
  })
  .await;
  let faster = OptionsPatch {
    set: BTreeMap::from([(
      "transfers.groups.leechers.upload.speed_limit".to_string(),
      json!(100_000),
    )]),
    unset: Vec::new(),
    expires_in: None,
  };
  api.patch_options(&faster).await.unwrap();
  tokio::time::timeout(PATIENCE, running)
    .await
    .expect("the first download ends")
    .unwrap()
    .unwrap();
  tokio::time::timeout(PATIENCE, again)
    .await
    .expect("the second download ends")
    .unwrap()
    .unwrap();
  assert_eq!(std::fs::read(&into).unwrap(), content(small));
  for (user, filename, size) in [(&first, big_name, big), (&second, small_name, small)] {
    let upload = upload_when(&api, &user.username, filename, "succeeded", |t| {
      t.state == succeeded()
    })
    .await;
    assert_eq!(upload.bytes_transferred, size as u64);
    assert!(upload.enqueued_at.is_some() && upload.started_at.is_some());
  }
  assert!(
    api.application().await.unwrap().server.is_logged_in,
    "no reconnect"
  );
  assert_eq!(
    metric(&api, "reseek_transfers_uploads_completed_succeeded").await,
    2.0
  );
  assert_eq!(
    metric(&api, "reseek_transfers_uploads_completed_failed").await,
    1.0
  );
  assert_eq!(
    metric(&api, "reseek_transfers_uploads_completed_bytes").await,
    (big + small) as f64
  );
  assert_eq!(metric(&api, "reseek_enqueue_requests_accepted").await, 3.0);
  let complete = events(&api, "UploadFileComplete", 2).await;
  assert!(
    complete
      .iter()
      .any(|data| data["remoteFilename"] == small_name
        && data["localFilename"]
          .as_str()
          .unwrap()
          .ends_with("small.flac")),
    "{complete:?}"
  );
}

#[tokio::test]
async fn an_upload_that_failed_once_started_is_queued_anew_when_the_peer_asks_again() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let album = dir.path().join("share").join("album");
  std::fs::create_dir_all(&album).unwrap();
  let size = 100_000;
  file(&album, "slow.flac", size);
  let name = soulfind::username("daemon");
  // the test's users share nothing, so they are leechers, and 1 KiB/s keeps the upload going
  let config = "[transfers.groups.leechers.upload]\nspeed_limit = 1\n";
  let (_daemon, api) = sharing(dir.path(), &server, &name, config, 1).await;
  let mut other = peer(&server, "again").await;
  let remote = "share\\album\\slow.flac";

  let (failing, _) = download(&other, &name, remote, size);
  let first = upload_when(&api, &other.username, remote, "under way", |t| {
    t.state == TransferStates::InProgress && t.bytes_transferred > 0
  })
  .await;
  // the shared file shrinks under the upload, which then fails and tells the peer
  let path = album.join("slow.flac");
  std::fs::File::options()
    .write(true)
    .open(&path)
    .unwrap()
    .set_len(10)
    .unwrap();
  let failed = tokio::time::timeout(PATIENCE, failing)
    .await
    .expect("the download ends")
    .unwrap();
  assert!(failed.is_err(), "{failed:?}");
  other
    .event(|event| matches!(event, Event::DownloadFailed { .. }).then_some(()))
    .await;
  // the file is whole again, and the peer asks again as soon as it is told, as clients do
  file(&album, "slow.flac", size);
  let (_again, _) = download(&other, &name, remote, size);
  let second = upload_when(&api, &other.username, remote, "queued anew", |t| {
    t.id != first.id
  })
  .await;
  assert!(!second.removed);
  let query = UploadsQuery {
    include_removed: true,
    ..UploadsQuery::default()
  };
  let ended = api
    .uploads(&query)
    .await
    .unwrap()
    .into_iter()
    .flat_map(|user| user.directories)
    .flat_map(|directory| directory.files)
    .find(|t| t.id == first.id)
    .unwrap();
  assert!(ended.removed, "the new request takes its place");
  assert!(ended.state.contains(TransferStates::Completed));
}
