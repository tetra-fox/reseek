//! Uploads to a scripted downloader, for what a real downloader does not do on demand: refusing,
//! never answering, asking to resume past the end or with Soulseek NS's broken offset, closing
//! early or never, the upload's turn, slot and source misbehaving, and the upload canceled at
//! each stage, with what the downloader is told afterwards.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crate::support::peer::PeerSide;
use crate::support::scripted::{BOB, Scripted};
use crate::support::transfers::{self, content, file, scratch, states};
use crate::support::{Logs, PATIENCE, listening_options};
use soulseek_client::{
  ClientError, ConnectionErrorKind, Event, Governor, InputFactory, Transfer, TransferOutcome,
  TransferState, UploadInput, UploadRequest,
};
use soulseek_proto::peer::{QueueDownload, TransferResponse, UploadDenied, UploadFailed};
use soulseek_proto::server::{GetPeerAddressRequest, GetPeerAddressResponse};
use soulseek_proto::{PeerMessage, ServerRequest, ServerResponse, TransferDirection};
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const SIZE: usize = 50_000;

/// An upload to bob running on a task, with what cancels it
struct Running {
  task: JoinHandle<Result<Transfer, ClientError>>,
  cancel: CancellationToken,
  source: PathBuf,
}

impl Running {
  async fn result(self) -> Result<Transfer, ClientError> {
    tokio::time::timeout(PATIENCE, self.task)
      .await
      .expect("the upload ends")
      .unwrap()
  }
}

/// starts uploading a file of [`SIZE`] bytes to bob as `filename`, with the request shaped by
/// `shape`
fn upload(scripted: &Scripted, filename: &str, shape: impl FnOnce(&mut UploadRequest)) -> Running {
  let source = file(&scratch(), "upload", SIZE);
  let mut request = UploadRequest::new(BOB, filename, UploadInput::File(source.clone()));
  shape(&mut request);
  let cancel = CancellationToken::new();
  let client = scripted.client().clone();
  let task = tokio::spawn({
    let cancel = cancel.clone();
    async move { client.upload(request, &cancel).await }
  });
  Running {
    task,
    cancel,
    source,
  }
}

/// bob reads the client's offer to send `filename` and returns its token
async fn offered(bob: &mut PeerSide, filename: &str) -> u32 {
  let PeerMessage::TransferRequest(offer) = bob.read_peer().await else {
    panic!("an upload offer");
  };
  assert_eq!(offer.direction, TransferDirection::Upload);
  assert_eq!(offer.filename.as_str(), filename);
  assert_eq!(offer.file_size, Some(SIZE as u64));
  offer.token
}

async fn allow(bob: &mut PeerSide, token: u32) {
  bob
    .send(
      &PeerMessage::TransferResponse(TransferResponse::Allowed {
        token,
        file_size: None,
      })
      .encode(),
    )
    .await;
}

/// bob takes the upload offered as `filename` up to the transfer connection, which it returns
async fn accepted(scripted: &mut Scripted, bob: &mut PeerSide, filename: &str) -> PeerSide {
  let token = offered(bob, filename).await;
  allow(bob, token).await;
  let (transfer, named) = scripted.accept_transfer().await;
  assert_eq!(named, token);
  transfer
}

/// bob asks to start at `start` and reads the rest of the file
async fn receive(transfer: &mut PeerSide, start: u64) {
  transfer.send(&start.to_le_bytes()).await;
  let rest = transfer.raw(SIZE - start as usize).await;
  assert_eq!(rest, content(SIZE)[start as usize..]);
}

/// an upload to bob up to the point bob has the offer, with bob's message connection
async fn offered_upload(scripted: &mut Scripted, filename: &str) -> (Running, PeerSide) {
  let running = upload(scripted, filename, |_| {});
  let bob = scripted.connected().await;
  (running, bob)
}

/// a governor that lets the first 1000 bytes through and nothing after, so the upload stays in
/// progress
fn first_chunk_only() -> Governor {
  let granted = Arc::new(AtomicU32::new(0));
  Arc::new(move |_, wanted, _| {
    let first = granted.fetch_add(1, Ordering::SeqCst) == 0;
    Box::pin(async move {
      if first {
        wanted.min(1000)
      } else {
        std::future::pending().await
      }
    })
  })
}

fn told_failed(message: PeerMessage, filename: &str) -> bool {
  message
    == PeerMessage::UploadFailed(UploadFailed {
      filename: filename.into(),
    })
}

fn told_canceled(message: PeerMessage, filename: &str) -> bool {
  message
    == PeerMessage::UploadDenied(UploadDenied {
      filename: filename.into(),
      reason: "Cancelled".into(),
    })
}

#[tokio::test]
async fn an_upload_ends_when_the_downloader_closes_after_the_last_byte() {
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = offered_upload(&mut scripted, "@@share\\a.mp3").await;
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\a.mp3").await;
  receive(&mut transfer, 0).await;
  let started = tokio::time::Instant::now();
  drop(transfer);
  let done = running.result().await.unwrap();
  assert!(
    started.elapsed() < Duration::from_secs(2),
    "no linger once closed"
  );
  assert_eq!(done.bytes_transferred, SIZE as u64);
  assert!(
    bob.quiet_for(Duration::from_millis(200)).await,
    "nothing to tell"
  );
}

#[tokio::test]
async fn a_downloader_that_refuses_ends_the_upload_untold_and_one_that_never_answers_hears_it_failed()
 {
  // a refusal, "Queued" included, since a downloader may not queue us
  for reason in ["Queued", "Cancelled"] {
    let mut scripted = Scripted::start().await;
    let (running, mut bob) = offered_upload(&mut scripted, "@@share\\r.mp3").await;
    let token = offered(&mut bob, "@@share\\r.mp3").await;
    bob
      .send(
        &PeerMessage::TransferResponse(TransferResponse::Denied {
          token,
          message: reason.into(),
        })
        .encode(),
      )
      .await;
    let error = running.result().await.unwrap_err();
    assert!(
      matches!(&error, ClientError::TransferRejected(given) if given == reason),
      "{error:?}"
    );
    // told it failed, it would queue the file it just refused
    assert!(
      bob.quiet_for(Duration::from_millis(200)).await,
      "a refusal is not answered"
    );
  }

  // no answer
  let mut options = listening_options();
  options.transfer_response_timeout = Duration::from_millis(200);
  let mut scripted = Scripted::with_options(options).await;
  let mut events = scripted.client().subscribe();
  let (running, mut bob) = offered_upload(&mut scripted, "@@share\\t.mp3").await;
  offered(&mut bob, "@@share\\t.mp3").await;
  assert!(matches!(
    running.result().await.unwrap_err(),
    ClientError::Timeout(_)
  ));
  let (_, last) = states(&mut events, TransferDirection::Upload).await;
  assert_eq!(
    last.state,
    TransferState::Completed(TransferOutcome::TimedOut)
  );
  assert!(told_failed(bob.read_peer().await, "@@share\\t.mp3"));
}

#[tokio::test]
async fn a_downloader_that_has_the_whole_file_gets_nothing_more() {
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = offered_upload(&mut scripted, "@@share\\whole.mp3").await;
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\whole.mp3").await;
  receive(&mut transfer, SIZE as u64).await;
  drop(transfer);
  assert_eq!(
    running.result().await.unwrap().bytes_transferred,
    SIZE as u64
  );
}

#[tokio::test]
async fn an_offset_past_the_end_fails_the_upload_and_the_broken_one_is_logged_once() {
  let logs = Logs::capture();
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = offered_upload(&mut scripted, "@@share\\past.mp3").await;
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\past.mp3").await;
  transfer.send(&(SIZE as u64 + 1).to_le_bytes()).await;
  let error = running.result().await.unwrap_err();
  assert!(
    matches!(&error, ClientError::Transfer { message, .. } if message.starts_with("Requested start offset of")),
    "{error:?}"
  );
  assert!(told_failed(bob.read_peer().await, "@@share\\past.mp3"));
  // the transfer connection is closed for that reason, not left to its inactivity timeout
  logs
    .wait_for(&["connection closed", "Requested start offset of"])
    .await;

  // Soulseek NS resuming a file over 2 GiB, twice
  for name in ["@@share\\ns1.mp3", "@@share\\ns2.mp3"] {
    let running = upload(&scripted, name, |_| {});
    let mut transfer = accepted(&mut scripted, &mut bob, name).await;
    transfer.send(&u64::MAX.to_le_bytes()).await;
    assert!(matches!(
      running.result().await.unwrap_err(),
      ClientError::Transfer { .. }
    ));
    assert!(told_failed(bob.read_peer().await, name));
  }
  assert_eq!(
    logs
      .lines_with(&["Peer sent an invalid resume offset", "bob"])
      .len(),
    1
  );
}

#[tokio::test]
async fn a_failed_upload_tells_the_downloader_which_may_queue_the_file_again_at_once() {
  let mut options = listening_options();
  let mut requests = transfers::queueing(&mut options);
  let mut scripted = Scripted::with_options(options).await;
  let running = upload(&scripted, "@@share\\shrunk.mp3", |_| {});
  let mut bob = scripted.connected().await;
  let source = running.source.clone();
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\shrunk.mp3").await;
  std::fs::write(&source, content(SIZE / 2)).unwrap();
  transfer.send(&0u64.to_le_bytes()).await;
  let error = running.result().await.unwrap_err();
  assert!(matches!(error, ClientError::Transfer { .. }), "{error:?}");
  // told as soon as the upload's record is gone, so asking again finds no duplicate
  assert!(told_failed(bob.read_peer().await, "@@share\\shrunk.mp3"));
  bob
    .send(
      &PeerMessage::QueueDownload(QueueDownload {
        filename: "@@share\\shrunk.mp3".into(),
      })
      .encode(),
    )
    .await;
  let (username, filename) = transfers::asked(&mut requests).await;
  scripted
    .client()
    .enqueue_upload(
      UploadRequest::new(username, filename, UploadInput::File(source)),
      &CancellationToken::new(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_downloader_that_closes_or_resets_mid_file_left_and_is_told_nothing() {
  for reset in [false, true] {
    let mut scripted = Scripted::start().await;
    let running = upload(&scripted, "@@share\\left.mp3", |request| {
      request.options.governor = Some(first_chunk_only());
    });
    let mut bob = scripted.connected().await;
    let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\left.mp3").await;
    transfer.send(&0u64.to_le_bytes()).await;
    assert_eq!(transfer.raw(1000).await, content(1000));
    if reset {
      // with no linger the close resets the connection, as a close with unread bytes does
      socket2::SockRef::from(&transfer.stream)
        .set_linger(Some(Duration::ZERO))
        .unwrap();
    }
    drop(transfer);
    let error = running.result().await.unwrap_err();
    assert_eq!(
      error.to_string(),
      "Transfer failed: the downloader closed the connection",
      "reset: {reset}"
    );
    assert_eq!(
      error.connection_kind(),
      Some(ConnectionErrorKind::RemoteClosed)
    );
    // told it failed, it would queue the file it just let go of
    assert!(
      bob.quiet_for(Duration::from_millis(200)).await,
      "reset: {reset}"
    );
  }
}

#[tokio::test]
async fn a_downloader_that_resets_after_the_last_byte_fails_the_upload_and_is_told_nothing() {
  let mut scripted = Scripted::start().await;
  let written = Arc::new(Notify::new());
  let running = upload(&scripted, "@@share\\reset.mp3", |request| {
    let written = written.clone();
    request.options.progress_updated = Some(Arc::new(move |_, transfer| {
      if transfer.bytes_transferred == SIZE as u64 {
        written.notify_one();
      }
    }));
  });
  let mut bob = scripted.connected().await;
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\reset.mp3").await;
  transfer.send(&0u64.to_le_bytes()).await;
  assert_eq!(transfer.raw(SIZE - 10).await, content(SIZE - 10));
  // every byte is written, so the upload waits for the downloader to close; it resets instead,
  // its last bytes unread
  written.notified().await;
  socket2::SockRef::from(&transfer.stream)
    .set_linger(Some(Duration::ZERO))
    .unwrap();
  drop(transfer);
  let error = running.result().await.unwrap_err();
  assert_eq!(
    error.to_string(),
    "Transfer failed: the downloader reset the connection after the last byte was written"
  );
  assert!(bob.quiet_for(Duration::from_millis(200)).await);
}

#[tokio::test]
async fn an_upload_that_fails_after_the_server_session_was_lost_still_tells_the_downloader() {
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = offered_upload(&mut scripted, "@@share\\outage.mp3").await;
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\outage.mp3").await;
  // the server goes away without the application asking to disconnect
  scripted.session.server.stream.shutdown().await.unwrap();
  scripted
    .session
    .event(|event| matches!(event, Event::Disconnected { .. }).then_some(()))
    .await;
  std::fs::write(&running.source, content(SIZE / 2)).unwrap();
  transfer.send(&0u64.to_le_bytes()).await;
  let error = running.result().await.unwrap_err();
  assert!(matches!(error, ClientError::Transfer { .. }), "{error:?}");
  assert!(told_failed(bob.read_peer().await, "@@share\\outage.mp3"));
}

#[tokio::test]
async fn a_downloader_that_never_closes_is_closed_after_the_linger_and_the_upload_stands() {
  let logs = Logs::capture();
  let mut scripted = Scripted::start().await;
  let running = upload(&scripted, "@@share\\linger.mp3", |request| {
    request.options.maximum_linger_time = Duration::from_millis(200);
  });
  let mut bob = scripted.connected().await;
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\linger.mp3").await;
  receive(&mut transfer, 0).await;
  // kept open; the client closes it
  transfer.closed().await;
  assert_eq!(
    running.result().await.unwrap().bytes_transferred,
    SIZE as u64
  );
  assert_eq!(
    logs
      .lines_with(&["exceeding the maximum linger time"])
      .len(),
    1
  );

  // canceling during the linger leaves a delivered upload succeeded, and the peer untold
  let running = upload(&scripted, "@@share\\linger2.mp3", |request| {
    request.options.maximum_linger_time = Duration::from_secs(30);
  });
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\linger2.mp3").await;
  receive(&mut transfer, 0).await;
  tokio::time::sleep(Duration::from_millis(100)).await;
  running.cancel.cancel();
  assert!(running.result().await.unwrap().state.succeeded());
  assert!(bob.quiet_for(Duration::from_millis(200)).await);
}

#[tokio::test]
async fn an_upload_canceled_at_any_stage_tells_the_downloader_to_drop_it() {
  // waiting for its turn behind another upload to bob
  let mut scripted = Scripted::start().await;
  let (_first, mut bob) = offered_upload(&mut scripted, "@@share\\first.mp3").await;
  offered(&mut bob, "@@share\\first.mp3").await;
  let mut events = scripted.client().subscribe();
  let second = upload(&scripted, "@@share\\second.mp3", |_| {});
  let ended = tokio::time::timeout(
    Duration::from_millis(300),
    states(&mut events, TransferDirection::Upload),
  )
  .await;
  assert!(ended.is_err(), "the second upload stays queued");
  assert_eq!(
    scripted
      .client()
      .uploads()
      .iter()
      .find(|upload| upload.filename.as_str() == "@@share\\second.mp3")
      .unwrap()
      .state,
    TransferState::QueuedLocally
  );
  second.cancel.cancel();
  assert!(matches!(
    second.result().await.unwrap_err(),
    ClientError::Canceled
  ));
  assert!(told_canceled(bob.read_peer().await, "@@share\\second.mp3"));

  // waiting for the downloader's answer
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = offered_upload(&mut scripted, "@@share\\asked.mp3").await;
  offered(&mut bob, "@@share\\asked.mp3").await;
  running.cancel.cancel();
  assert!(matches!(
    running.result().await.unwrap_err(),
    ClientError::Canceled
  ));
  assert!(told_canceled(bob.read_peer().await, "@@share\\asked.mp3"));

  // waiting for the downloader to say where to start
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = offered_upload(&mut scripted, "@@share\\init.mp3").await;
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\init.mp3").await;
  running.cancel.cancel();
  assert!(matches!(
    running.result().await.unwrap_err(),
    ClientError::Canceled
  ));
  transfer.closed().await;
  assert!(told_canceled(bob.read_peer().await, "@@share\\init.mp3"));

  // with bytes moving, held back by a governor that allows nothing after the first chunk
  let mut scripted = Scripted::start().await;
  let running = upload(&scripted, "@@share\\moving.mp3", |request| {
    request.options.governor = Some(first_chunk_only());
  });
  let mut bob = scripted.connected().await;
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\moving.mp3").await;
  transfer.send(&0u64.to_le_bytes()).await;
  assert_eq!(transfer.raw(1000).await, content(1000));
  running.cancel.cancel();
  let error = running.result().await.unwrap_err();
  assert!(matches!(error, ClientError::Canceled), "{error:?}");
  transfer.closed().await;
  assert!(told_canceled(bob.read_peer().await, "@@share\\moving.mp3"));
}

#[tokio::test]
async fn the_slot_policy_admits_the_upload_and_gets_the_slot_back_once() {
  // a slot policy that fails holds nothing, so nothing is handed back
  let released = Arc::new(AtomicU32::new(0));
  let mut scripted = Scripted::start().await;
  let running = upload(&scripted, "@@share\\noslot.mp3", |request| {
    request.options.slot_awaiter = Some(Arc::new(|_, _| {
      Box::pin(async { Err(ClientError::client("queue on fire", None)) })
    }));
    let released = released.clone();
    request.options.slot_released = Some(Arc::new(move |_| {
      released.fetch_add(1, Ordering::SeqCst);
    }));
  });
  // bob, who asked for the file, hears it failed and may ask again
  let mut bob = scripted.connected().await;
  assert!(told_failed(bob.read_peer().await, "@@share\\noslot.mp3"));
  let error = running.result().await.unwrap_err();
  assert!(
    matches!(&error, ClientError::Transfer { message, .. } if message.starts_with("Failed to acquire an upload slot")),
    "{error:?}"
  );
  assert_eq!(released.load(Ordering::SeqCst), 0);

  // one that admits it gets the slot back once the upload ends
  let running = upload(&scripted, "@@share\\slot.mp3", |request| {
    request.options.slot_awaiter = Some(Arc::new(|_, _| Box::pin(async { Ok(()) })));
    let released = released.clone();
    request.options.slot_released = Some(Arc::new(move |_| {
      released.fetch_add(1, Ordering::SeqCst);
    }));
  });
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\slot.mp3").await;
  receive(&mut transfer, 0).await;
  drop(transfer);
  running.result().await.unwrap();
  assert_eq!(released.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_source_that_changed_or_ends_early_fails_the_upload() {
  // the file shrank while the upload waited
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = offered_upload(&mut scripted, "@@share\\shrunk.mp3").await;
  std::fs::write(&running.source, content(SIZE / 2)).unwrap();
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\shrunk.mp3").await;
  transfer.send(&0u64.to_le_bytes()).await;
  let error = running.result().await.unwrap_err();
  assert!(
    matches!(&error, ClientError::Transfer { message, .. } if message.starts_with("file changed size")),
    "{error:?}"
  );
  assert!(told_failed(bob.read_peer().await, "@@share\\shrunk.mp3"));

  // a stream that ends before the size it claimed
  let open: InputFactory = Box::new(|_| {
    Box::pin(async {
      let source: Box<dyn soulseek_client::UploadSource> =
        Box::new(std::io::Cursor::new(content(SIZE - 10)));
      Ok(source)
    })
  });
  let request = UploadRequest::new(
    BOB,
    "@@share\\short.mp3",
    UploadInput::Stream {
      size: SIZE as u64,
      open,
    },
  );
  let client = scripted.client().clone();
  let task = tokio::spawn(async move { client.upload(request, &CancellationToken::new()).await });
  let mut transfer = accepted(&mut scripted, &mut bob, "@@share\\short.mp3").await;
  transfer.send(&0u64.to_le_bytes()).await;
  let error = task.await.unwrap().unwrap_err();
  assert!(
    matches!(&error, ClientError::Transfer { message, .. } if message.starts_with("input ended prematurely")),
    "{error:?}"
  );
}

#[tokio::test]
async fn an_upload_is_queued_at_once_and_once_per_file() {
  let scripted = Scripted::start().await;
  let source = file(&scratch(), "upload", SIZE);
  let request = || UploadRequest::new(BOB, "@@share\\q.mp3", UploadInput::File(source.clone()));
  let task = scripted
    .client()
    .enqueue_upload(request(), &CancellationToken::new())
    .await
    .unwrap();
  assert_eq!(
    scripted.client().uploads()[0].state,
    TransferState::QueuedLocally
  );
  assert!(matches!(
    scripted
      .client()
      .enqueue_upload(request(), &CancellationToken::new())
      .await,
    Err(ClientError::DuplicateTransfer { .. })
  ));
  drop(task);
  let missing = UploadRequest::new(
    BOB,
    "@@share\\m.mp3",
    UploadInput::File(scratch().join("nope")),
  );
  assert!(matches!(
    scripted
      .client()
      .upload(missing, &CancellationToken::new())
      .await,
    Err(ClientError::NotFound(_))
  ));
}

#[tokio::test]
async fn an_upload_to_a_user_who_is_offline_fails_without_telling_anyone() {
  let mut scripted = Scripted::start().await;
  let running = upload(&scripted, "@@share\\off.mp3", |_| {});
  scripted
    .session
    .server
    .request_matching(|request| match request {
      ServerRequest::GetPeerAddress(GetPeerAddressRequest { username }) if username == BOB => {
        Some(())
      }
      _ => None,
    })
    .await;
  scripted
    .session
    .server
    .send(&ServerResponse::GetPeerAddress(GetPeerAddressResponse {
      username: BOB.into(),
      ip: std::net::Ipv4Addr::UNSPECIFIED,
      port: 0,
      obfuscation: None,
    }))
    .await;
  let started = tokio::time::Instant::now();
  let error = running.result().await.unwrap_err();
  assert!(matches!(error, ClientError::UserOffline(_)), "{error:?}");
  assert!(
    started.elapsed() < Duration::from_secs(2),
    "no lookup for a courtesy"
  );
}
