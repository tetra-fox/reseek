//! Downloads by the daemon over soulfind from a client that queues every request and uploads
//! when the test says: a file lands at its destination with its path recorded and its events
//! raised, a move that fails keeps the download, a cancel and a block end queued downloads,
//! a download a stop interrupted is resumed after the next start and finishes, one whose every
//! byte arrived before the stop is moved without asking the peer again, and a finished download
//! is analyzed when the options ask for it.

mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use api_client::{Client, LocalFile};
use api_types::analysis::{AnalysisReportQuery, AudioAnalysis};
use api_types::files::ContentQuery;
use api_types::options::OptionsPatch;
use api_types::transfers::{
  BatchOptions, EnqueueDownloadBatchItem, EnqueueDownloadBatchRequest, QueueDownloadRequest,
  Transfer, TransferDirection, TransferStates,
};
use serde_json::json;
use soulseek_client::{ClientOptions, UploadInput, UploadRequest};
use support::soulfind::{self, Soulfind, User};
use support::transfers::{self, Asked, asked, content, file, scratch};
use support::{
  Daemon, PATIENCE, completed, daemon, download_when, enqueue, events, free_port, logged_in,
  signed_in, until,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const SIZE: usize = 200_000;
const ALBUM: &str = "@@share\\album\\01 song.flac";

/// An uploader that queues every request and hands it to the test.
async fn uploader(server: &Arc<Soulfind>) -> (User, Asked) {
  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  let requests = transfers::queueing(&mut options);
  (server.login("up", options).await, requests)
}

/// Starts uploading `path` for the next request the uploader is asked, which must be for
/// `remote`.
async fn serve(uploader: &User, requests: &mut Asked, remote: &str, path: &Path) {
  let (username, filename) = asked(requests).await;
  assert_eq!(filename.as_str(), remote);
  let client = uploader.client.clone();
  let input = UploadInput::File(path.to_path_buf());
  tokio::spawn(async move {
    let request = UploadRequest::new(username, filename, input);
    client.upload(request, &CancellationToken::new()).await
  });
}

/// The daemon in `dir` for `server`, logged in, and its client.
async fn started(dir: &Path, server: &Arc<Soulfind>, name: &str) -> (Daemon, Client) {
  let daemon = daemon(dir, server, name, &[]);
  let api = signed_in(&daemon).await;
  until(&api, "logged in", logged_in).await;
  (daemon, api)
}

#[tokio::test]
async fn a_download_lands_at_its_destination_and_a_failed_move_keeps_it() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (uploader, mut requests) = uploader(&server).await;
  let dir = tempfile::tempdir().unwrap();
  let (_daemon, api) = started(dir.path(), &server, &soulfind::username("daemon")).await;
  let source = scratch();
  let one = file(&source, "one.flac", SIZE);
  let peer = uploader.username.clone();

  let id = enqueue(&api, &peer, ALBUM, SIZE as u64).await;
  serve(&uploader, &mut requests, ALBUM, &one).await;
  // a download reads as succeeded only once its file is in place
  let succeeded = |t: &Transfer| t.state == completed(TransferStates::Succeeded);
  let done = download_when(&api, &peer, id, "succeeded", succeeded).await;
  let landed = dir
    .path()
    .join("data")
    .join("downloads")
    .join("album")
    .join("01 song.flac");
  assert_eq!(done.local_path.as_deref(), Some(landed.to_str().unwrap()));
  assert_eq!(std::fs::read(&landed).unwrap(), content(SIZE));
  assert!(
    !dir
      .path()
      .join("data")
      .join("incomplete")
      .join(&peer)
      .join("album")
      .exists(),
    "the emptied incomplete directory goes"
  );
  assert_eq!((done.bytes_transferred, done.attempts), (SIZE as u64, 1));
  let entered: Vec<String> = done
    .state_history
    .unwrap()
    .iter()
    .map(|change| change.state.to_string())
    .collect();
  assert_eq!(
    entered,
    [
      "Queued, Locally",
      "Requested",
      "Queued, Remotely",
      "Initializing",
      "InProgress",
      "Completed, Succeeded"
    ]
  );
  let served = api
    .file_content(LocalFile::Download(&peer, id), &ContentQuery::default())
    .await
    .unwrap();
  assert_eq!(served.len(), SIZE);
  let data = events(&api, "DownloadFileComplete,DownloadDirectoryComplete", 2).await;
  assert!(
    data.iter().any(
      |d| d["localFilename"] == landed.to_str().unwrap() && d["transfer"]["id"] == id.to_string()
    ),
    "{data:?}"
  );
  assert!(
    data
      .iter()
      .any(|d| d["remoteDirectoryName"] == "@@share\\album" && d["username"] == peer),
    "{data:?}"
  );

  // a file stands where the directory would be made
  let blocked = "@@share\\in the way\\02 song.flac";
  std::fs::write(
    dir.path().join("data").join("downloads").join("in the way"),
    "x",
  )
  .unwrap();
  let id = enqueue(&api, &peer, blocked, SIZE as u64).await;
  serve(&uploader, &mut requests, blocked, &one).await;
  // every byte arrived, so the download succeeded with the move's error
  let kept = download_when(&api, &peer, id, "succeeded", succeeded).await;
  assert!(
    kept
      .exception
      .as_deref()
      .unwrap()
      .starts_with("Failed to move file 02 song.flac"),
    "{kept:?}"
  );
  assert_eq!(kept.local_path, None);
  let failed = events(&api, "DownloadFileFailed", 1).await;
  assert_eq!(failed[0]["stage"], "move");
  let incomplete = dir
    .path()
    .join("data")
    .join("incomplete")
    .join(&peer)
    .join("in the way")
    .join("02 song.flac");
  assert_eq!(failed[0]["localFilename"], incomplete.to_str().unwrap());
  assert!(incomplete.exists(), "the file stays where it arrived");
}

#[tokio::test]
async fn a_cancel_removes_once_ended_and_a_block_cancels_with_its_reason() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (uploader, mut requests) = uploader(&server).await;
  let dir = tempfile::tempdir().unwrap();
  let (_daemon, api) = started(dir.path(), &server, &soulfind::username("daemon")).await;
  let peer = uploader.username.clone();
  let queued = |t: &Transfer| t.state == TransferStates::Queued | TransferStates::Remotely;

  let canceled = enqueue(&api, &peer, "@@share\\a\\1.flac", 10).await;
  let blocked = enqueue(&api, &peer, "@@share\\a\\2.flac", 10).await;
  for _ in 0..2 {
    asked(&mut requests).await;
  }
  download_when(&api, &peer, canceled, "queued", queued).await;
  download_when(&api, &peer, blocked, "queued", queued).await;
  api
    .cancel_transfer(TransferDirection::Download, &peer, canceled, true)
    .await
    .unwrap();
  let ended = download_when(&api, &peer, canceled, "removed", |t| t.removed).await;
  assert_eq!(ended.state, completed(TransferStates::Cancelled));
  assert_eq!(ended.exception.as_deref(), Some("Operation cancelled"));
  let missing = api
    .cancel_transfer(TransferDirection::Download, &peer, Uuid::new_v4(), false)
    .await
    .unwrap_err();
  assert_eq!(missing.status().map(|s| s.as_u16()), Some(404));

  let block = serde_json::from_value(json!({ "scopes": ["downloads"] })).unwrap();
  api.block(&peer, &block).await.unwrap();
  let ended = download_when(&api, &peer, blocked, "canceled", |t| t.ended_at.is_some()).await;
  assert_eq!(ended.state, completed(TransferStates::Cancelled));
  assert_eq!(ended.exception.as_deref(), Some("Blocked"));
  let refused = api
    .enqueue(
      &peer,
      &[QueueDownloadRequest {
        filename: "@@share\\a\\3.flac".to_string(),
        size: 1,
      }],
    )
    .await
    .unwrap_err();
  assert_eq!(refused.status().map(|s| s.as_u16()), Some(409));
  assert!(
    events(&api, "DownloadFileFailed", 0).await.is_empty(),
    "a cancel raises nothing"
  );
}

#[tokio::test]
async fn a_batch_is_made_whole_or_not_at_all() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (uploader, mut requests) = uploader(&server).await;
  let dir = tempfile::tempdir().unwrap();
  let (_daemon, api) = started(dir.path(), &server, &soulfind::username("daemon")).await;
  let batch = |username: &str, id: Uuid| EnqueueDownloadBatchRequest {
    id: Some(id),
    search_id: None,
    username: username.to_string(),
    files: vec![EnqueueDownloadBatchItem {
      filename: "@@share\\b\\1.flac".to_string(),
      size: 10,
      attributes: Vec::new(),
    }],
    options: BatchOptions::default(),
  };
  let offline = Uuid::new_v4();
  let refused = api
    .enqueue_batch(&batch("nobody-logged-in", offline))
    .await
    .unwrap_err();
  assert_eq!(refused.status().map(|s| s.as_u16()), Some(404));
  let missing = api.batch(offline, false).await.unwrap_err();
  assert_eq!(
    missing.status().map(|s| s.as_u16()),
    Some(404),
    "no batch is left"
  );

  let id = Uuid::new_v4();
  let made = api
    .enqueue_batch(&batch(&uploader.username, id))
    .await
    .unwrap();
  assert_eq!(made.batch.transfers.len(), 1);
  asked(&mut requests).await;
  let again = api
    .enqueue_batch(&batch(&uploader.username, id))
    .await
    .unwrap_err();
  assert_eq!(again.status().map(|s| s.as_u16()), Some(409));
  assert_eq!(api.batch(id, true).await.unwrap().file_count, 1);
}

/// Enqueues [`ALBUM`] from `peer` in a batch of its own, waits for the peer to queue it and
/// stops the daemon. Returns the batch and the download.
async fn stopped_while_queued(
  mut daemon: Daemon,
  api: &Client,
  requests: &mut Asked,
  peer: &str,
) -> (Uuid, Uuid) {
  let batch = Uuid::new_v4();
  let request = EnqueueDownloadBatchRequest {
    id: Some(batch),
    search_id: None,
    username: peer.to_string(),
    files: vec![EnqueueDownloadBatchItem {
      filename: ALBUM.to_string(),
      size: SIZE as u64,
      attributes: Vec::new(),
    }],
    options: BatchOptions {
      destination: Some("Kept/Here".to_string()),
      external_id: None,
    },
  };
  let first = api.enqueue_batch(&request).await.unwrap().batch.transfers[0].id;
  asked(requests).await;
  download_when(api, peer, first, "queued", |t| {
    t.state == TransferStates::Queued | TransferStates::Remotely
  })
  .await;
  api.shut_down().await.unwrap();
  let stopping = Instant::now();
  while daemon.process.child.try_wait().unwrap().is_none() {
    assert!(stopping.elapsed() < PATIENCE, "the daemon stops");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
  (batch, first)
}

/// Waits for the download that takes over `first` in `batch` after a start to succeed, checks
/// that its file landed under the daemon in `dir` as the batch asked, and that `first` ended
/// with the stop and is superseded.
async fn resumed(api: &Client, dir: &Path, peer: &str, batch: Uuid, first: Uuid) {
  let started_at = Instant::now();
  let resumed = loop {
    let batch = api.batch(batch, true).await.unwrap();
    if let Some(resumed) = batch
      .transfers
      .iter()
      .find(|t| t.id != first && t.local_path.is_some())
    {
      break resumed.clone();
    }
    assert!(started_at.elapsed() < PATIENCE * 2, "{batch:?}");
    tokio::time::sleep(Duration::from_millis(50)).await;
  };
  assert_eq!(resumed.state, completed(TransferStates::Succeeded));
  assert_eq!(resumed.batch_id, Some(batch), "resumed in its batch");
  let landed = dir
    .join("data")
    .join("downloads")
    .join("Kept")
    .join("Here")
    .join("01 song.flac");
  assert_eq!(
    resumed.local_path.as_deref(),
    Some(landed.to_str().unwrap())
  );
  assert_eq!(std::fs::read(&landed).unwrap(), content(SIZE));
  let superseded = api
    .transfer(TransferDirection::Download, peer, first, false)
    .await
    .unwrap();
  assert_eq!(superseded.state, completed(TransferStates::Errored));
  assert_eq!(
    superseded.exception.as_deref(),
    Some("Application shut down")
  );
  assert!(superseded.removed, "the old record is superseded");
}

#[tokio::test]
async fn a_download_a_stop_interrupted_resumes_after_the_next_start() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (uploader, mut requests) = uploader(&server).await;
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let (daemon, api) = started(dir.path(), &server, &name).await;
  let peer = uploader.username.clone();
  let (batch, first) = stopped_while_queued(daemon, &api, &mut requests, &peer).await;

  let (_daemon, api) = started(dir.path(), &server, &name).await;
  let source = file(&scratch(), "one.flac", SIZE);
  serve(&uploader, &mut requests, ALBUM, &source).await;
  resumed(&api, dir.path(), &peer, batch, first).await;
}

#[tokio::test]
async fn a_download_whole_at_a_stop_is_moved_at_the_next_start_without_asking_again() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (uploader, mut requests) = uploader(&server).await;
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let (daemon, api) = started(dir.path(), &server, &name).await;
  let peer = uploader.username.clone();
  let (batch, first) = stopped_while_queued(daemon, &api, &mut requests, &peer).await;
  // every byte arrived and the stop came before the move
  let incomplete = dir
    .path()
    .join("data")
    .join("incomplete")
    .join(&peer)
    .join("album");
  std::fs::create_dir_all(&incomplete).unwrap();
  file(&incomplete, "01 song.flac", SIZE);

  let (_daemon, api) = started(dir.path(), &server, &name).await;
  resumed(&api, dir.path(), &peer, batch, first).await;
  assert!(requests.try_recv().is_err(), "the peer is not asked again");
}

/// The analysis report of the download `id` from `username`, `None` before there is one.
async fn report(api: &Client, username: &str, id: Uuid) -> Option<AudioAnalysis> {
  let query = AnalysisReportQuery { windows: false };
  match api
    .analysis(LocalFile::Download(username, id), &query)
    .await
  {
    Ok(report) => Some(report),
    Err(e) if e.status() == Some(reqwest::StatusCode::NOT_FOUND) => None,
    Err(e) => panic!("{e}"),
  }
}

/// The analysis report of the download `id` from `username` once `ready` holds for it.
async fn report_when(
  api: &Client,
  username: &str,
  id: Uuid,
  ready: impl Fn(&AudioAnalysis) -> bool,
) -> AudioAnalysis {
  let started = Instant::now();
  loop {
    if let Some(report) = report(api, username, id).await
      && ready(&report)
    {
      return report;
    }
    assert!(started.elapsed() < PATIENCE * 2, "{id} never analyzed");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

#[tokio::test]
async fn a_finished_download_is_analyzed_while_the_option_is_on() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (uploader, mut requests) = uploader(&server).await;
  let dir = tempfile::tempdir().unwrap();
  let (_daemon, api) = started(dir.path(), &server, &soulfind::username("daemon")).await;
  let source = scratch();
  let one = file(&source, "one.flac", SIZE);
  let peer = uploader.username.clone();
  let set = |on: bool| OptionsPatch {
    set: BTreeMap::from([("analysis.on_download".to_string(), json!(on))]),
    unset: Vec::new(),
    expires_in: None,
  };
  let mut download = async |name: &str| {
    let remote = format!("@@share\\analyzed\\{name}");
    let id = enqueue(&api, &peer, &remote, SIZE as u64).await;
    serve(&uploader, &mut requests, &remote, &one).await;
    download_when(&api, &peer, id, "moved", |t| t.local_path.is_some()).await;
    id
  };

  // the one worker takes files in the order they were queued, so a file queued before one
  // that was analyzed would have its report by then
  let off = download("01 off.flac").await;
  api.patch_options(&set(true)).await.unwrap();
  let on = download("02 on.flac").await;
  report_when(&api, &peer, on, |_| true).await;
  assert!(
    report(&api, &peer, off).await.is_none(),
    "analyzed while off"
  );

  let other = download("03 other.ogg").await;
  let after = download("04 after.flac").await;
  report_when(&api, &peer, after, |_| true).await;
  assert!(
    report(&api, &peer, other).await.is_none(),
    "an extension not listed was analyzed"
  );

  api.patch_options(&set(false)).await.unwrap();
  let later = download("05 later.flac").await;
  let before = report(&api, &peer, after).await.unwrap().analyzed_at;
  api
    .run_analysis(LocalFile::Download(&peer, after), true)
    .await
    .unwrap();
  report_when(&api, &peer, after, |r| r.analyzed_at > before).await;
  assert!(
    report(&api, &peer, later).await.is_none(),
    "analyzed after the option was turned off"
  );
}
