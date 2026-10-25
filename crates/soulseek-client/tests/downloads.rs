//! Downloads between two clients on soulfind, the uploader queueing every request and starting
//! the upload when the test says: asked for with `QueueDownload` or with a transfer request,
//! queued and then started, resumed from an offset, returned early once queued, and told its
//! place in the queue.

mod support;

use std::sync::Arc;
use std::time::Duration;

use soulseek_client::{
  ClientOptions, DownloadOutput, DownloadRequest, Event, RequestStyle, TransferState, UploadInput,
  UploadRequest,
};
use soulseek_proto::TransferDirection;
use support::soulfind::{self, User};
use support::transfers::{self, Asked, asked, content, file, scratch, states};
use support::{PATIENCE, free_port};
use tokio_util::sync::CancellationToken;

const SIZE: usize = 300_000;
const REMOTE: &str = "@@share\\album\\01 song.flac";

fn listening() -> ClientOptions {
  ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  }
}

/// a downloader and an uploader that queues every request the downloader makes
async fn pair(server: &Arc<soulfind::Soulfind>) -> (User, User, Asked) {
  let mut options = listening();
  let requests = transfers::queueing(&mut options);
  let uploader = server.login("up", options).await;
  let downloader = server.login("down", listening()).await;
  (downloader, uploader, requests)
}

/// starts the upload of `path` the downloader asked for
async fn serve(uploader: &User, requests: &mut Asked, path: std::path::PathBuf, start: bool) {
  let (username, filename) = asked(requests).await;
  assert_eq!(filename.as_str(), REMOTE);
  if start {
    let client = uploader.client.clone();
    tokio::spawn(async move {
      let request = UploadRequest::new(username, filename, UploadInput::File(path));
      client.upload(request, &CancellationToken::new()).await
    });
  }
}

#[tokio::test]
async fn a_queued_download_goes_through_every_state_and_arrives_whole() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (mut downloader, mut uploader, mut requests) = pair(&server).await;
  let dir = scratch();
  let source = file(&dir, "source.flac", SIZE);
  let destination = dir.join("destination.flac");
  let request = DownloadRequest::new(
    &uploader.username,
    REMOTE,
    SIZE as u64,
    DownloadOutput::File(destination.clone()),
  );
  let client = downloader.client.clone();
  let download =
    tokio::spawn(async move { client.download(request, &CancellationToken::new()).await });
  serve(&uploader, &mut requests, source, true).await;
  let (downloaded, transfer) = states(&mut downloader.events, TransferDirection::Download).await;
  assert_eq!(downloaded, transfers::queued_download());
  assert_eq!(transfer.bytes_transferred, SIZE as u64);
  assert!(transfer.remote_token.is_some());
  let (uploaded, _) = states(&mut uploader.events, TransferDirection::Upload).await;
  assert_eq!(uploaded, transfers::upload());
  let finished = download.await.unwrap().unwrap();
  assert_eq!(
    finished.state,
    TransferState::COMPLETED | TransferState::SUCCEEDED
  );
  assert_eq!(std::fs::read(destination).unwrap(), content(SIZE));
  assert!(downloader.client.downloads().is_empty());
}

#[tokio::test]
async fn a_download_asked_with_a_transfer_request_is_queued_by_the_answer() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (mut downloader, uploader, mut requests) = pair(&server).await;
  let dir = scratch();
  let source = file(&dir, "source.flac", SIZE);
  let mut request = DownloadRequest::new(
    &uploader.username,
    REMOTE,
    SIZE as u64,
    DownloadOutput::File(dir.join("destination.flac")),
  );
  request.options.request_style = RequestStyle::TransferRequest;
  let client = downloader.client.clone();
  let download =
    tokio::spawn(async move { client.download(request, &CancellationToken::new()).await });
  serve(&uploader, &mut requests, source, true).await;
  let (downloaded, _) = states(&mut downloader.events, TransferDirection::Download).await;
  assert_eq!(downloaded, transfers::queued_download());
  download.await.unwrap().unwrap();
  assert_eq!(
    std::fs::read(dir.join("destination.flac")).unwrap(),
    content(SIZE)
  );
}

#[tokio::test]
async fn a_resumed_download_keeps_what_it_has_and_fetches_the_rest() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (mut downloader, mut uploader, mut requests) = pair(&server).await;
  let dir = scratch();
  let source = file(&dir, "source.flac", SIZE);
  let destination = dir.join("destination.flac");
  // the first 100 000 bytes arrived before, and a stray tail after them goes
  let mut partial = content(100_000);
  partial.extend([0xee; 5000]);
  std::fs::write(&destination, partial).unwrap();
  let mut request = DownloadRequest::new(
    &uploader.username,
    REMOTE,
    SIZE as u64,
    DownloadOutput::File(destination.clone()),
  );
  request.start_offset = 100_000;
  let client = downloader.client.clone();
  let download =
    tokio::spawn(async move { client.download(request, &CancellationToken::new()).await });
  serve(&uploader, &mut requests, source, true).await;
  let (_, uploaded) = states(&mut uploader.events, TransferDirection::Upload).await;
  assert_eq!(uploaded.start_offset, 100_000);
  assert_eq!(uploaded.bytes_transferred, SIZE as u64);
  let (_, downloaded) = states(&mut downloader.events, TransferDirection::Download).await;
  assert_eq!(downloaded.start_offset, 100_000);
  assert_eq!(downloaded.bytes_transferred, SIZE as u64);
  download.await.unwrap().unwrap();
  assert_eq!(std::fs::read(destination).unwrap(), content(SIZE));
}

#[tokio::test]
async fn a_resume_from_a_file_shorter_than_the_offset_is_refused_before_asking() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (downloader, uploader, mut requests) = pair(&server).await;
  let dir = scratch();
  let destination = dir.join("destination.flac");
  std::fs::write(&destination, content(10)).unwrap();
  let mut request = DownloadRequest::new(
    &uploader.username,
    REMOTE,
    SIZE as u64,
    DownloadOutput::File(destination),
  );
  request.start_offset = 100;
  let error = downloader
    .client
    .download(request, &CancellationToken::new())
    .await
    .unwrap_err();
  assert!(
    matches!(&error, soulseek_client::ClientError::InvalidArgument(message) if message == "local file is shorter than the start offset"),
    "{error:?}"
  );
  assert!(requests.try_recv().is_err(), "the peer was never asked");
}

#[tokio::test]
async fn a_download_waits_in_the_queue_and_hears_its_place_until_the_upload_starts() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let mut options = listening();
  let mut requests = transfers::queueing(&mut options);
  options.resolvers.place_in_queue_resolver =
    Some(Arc::new(|_, _, _| Box::pin(async { Ok(Some(2)) })));
  let uploader = server.login("up", options).await;
  let mut downloader = server.login("down", listening()).await;
  let dir = scratch();
  let source = file(&dir, "source.flac", SIZE);
  let request = DownloadRequest::new(
    &uploader.username,
    REMOTE,
    SIZE as u64,
    DownloadOutput::File(dir.join("destination.flac")),
  );
  // returns once the uploader has the request queued
  let task = downloader
    .client
    .enqueue_download(request, &CancellationToken::new())
    .await
    .unwrap();
  let place = downloader
    .event(|event| match event {
      Event::PlaceInQueueReceived {
        filename, place, ..
      } if filename.as_str() == REMOTE => Some(place),
      _ => None,
    })
    .await;
  assert_eq!(place, 2);
  serve(&uploader, &mut requests, source.clone(), false).await;
  let asked = downloader
    .client
    .get_download_place_in_queue(&uploader.username, REMOTE, &CancellationToken::new())
    .await
    .unwrap();
  assert_eq!(asked, 2);
  // still queued at the uploader while it has not started
  tokio::time::sleep(Duration::from_millis(300)).await;
  let queued = downloader.client.downloads();
  assert_eq!(queued.len(), 1);
  assert_eq!(queued[0].state, TransferState::QUEUED_REMOTELY);
  let request = UploadRequest::new(&downloader.username, REMOTE, UploadInput::File(source));
  uploader
    .client
    .upload(request, &CancellationToken::new())
    .await
    .unwrap();
  let done = tokio::time::timeout(PATIENCE, task).await.unwrap().unwrap();
  assert_eq!(done.bytes_transferred, SIZE as u64);
}

#[tokio::test]
async fn a_download_cancelled_in_the_queue_refuses_the_upload_offered_later() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (downloader, mut uploader, mut requests) = pair(&server).await;
  let dir = scratch();
  let source = file(&dir, "source.flac", SIZE);
  let cancel = CancellationToken::new();
  let request = DownloadRequest::new(
    &uploader.username,
    REMOTE,
    SIZE as u64,
    DownloadOutput::File(dir.join("destination.flac")),
  );
  let task = downloader
    .client
    .enqueue_download(request, &cancel)
    .await
    .unwrap();
  serve(&uploader, &mut requests, source.clone(), false).await;
  cancel.cancel();
  let cancelled = task.await.unwrap_err();
  assert!(matches!(cancelled, soulseek_client::ClientError::Cancelled));
  assert!(!dir.join("destination.flac").exists(), "nothing was opened");
  let request = UploadRequest::new(&downloader.username, REMOTE, UploadInput::File(source));
  let refused = uploader
    .client
    .upload(request, &CancellationToken::new())
    .await
    .unwrap_err();
  assert!(
    matches!(&refused, soulseek_client::ClientError::TransferRejected(reason) if reason == "Cancelled"),
    "{refused:?}"
  );
  let (uploaded, _) = states(&mut uploader.events, TransferDirection::Upload).await;
  assert_eq!(
    uploaded.last(),
    Some(&(TransferState::COMPLETED | TransferState::REJECTED))
  );
}

#[tokio::test]
async fn a_peer_that_shares_nothing_refuses_the_download() {
  let Some(server) = soulfind::server() else {
    return;
  };
  // no upload queue at all, so nothing is shared
  let uploader = server.login("up", listening()).await;
  let downloader = server.login("down", listening()).await;
  for style in [RequestStyle::Queue, RequestStyle::TransferRequest] {
    let mut request = DownloadRequest::new(
      &uploader.username,
      REMOTE,
      SIZE as u64,
      DownloadOutput::File(scratch().join("destination.flac")),
    );
    request.options.request_style = style;
    let error = downloader
      .client
      .download(request, &CancellationToken::new())
      .await
      .unwrap_err();
    assert!(
      matches!(&error, soulseek_client::ClientError::TransferRejected(reason) if reason == "File not shared."),
      "{style:?}: {error:?}"
    );
  }
}
