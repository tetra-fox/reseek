//! Downloads from a scripted peer, for what a real uploader does not say on demand: refusing
//! with any reason, reporting a failure at each stage, naming another size or none, sending at
//! once, sending what does not decode; and how the download asks: governed, with fresh tokens,
//! once per file.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use soulseek_client::{
  ClientError, DownloadOutput, DownloadRequest, Event, RequestStyle, TransferState,
};
use soulseek_proto::peer::{TransferRequest, TransferResponse, UploadDenied, UploadFailed};
use soulseek_proto::{PeerMessage, TransferDirection};
use support::PATIENCE;
use support::scripted::{BOB, SIZE, Scripted, offer, queued, send_file};
use support::transfers::{content, scratch, states};
use tokio_util::sync::CancellationToken;

fn rejected(reason: &str) -> impl Fn(&ClientError) -> bool + '_ {
  move |error| matches!(error, ClientError::TransferRejected(given) if given == reason)
}

fn reported_failed(error: &ClientError) -> bool {
  matches!(error, ClientError::TransferReportedFailed(message) if message == "Download reported as failed by remote client")
}

#[tokio::test]
async fn a_refusal_with_any_reason_rejects_the_download() {
  let mut scripted = Scripted::start().await;
  // the reason is passed on as it is and never matched, so a few stand for every one
  let reasons = ["Cancelled", "File not shared.", ""];
  let mut bob = None;
  for (n, reason) in reasons.into_iter().enumerate() {
    let filename = format!("@@share\\refused {n}.mp3");
    let running = scripted.download(&filename, |_| {});
    // the first download connects; the rest find the connection cached
    if bob.is_none() {
      bob = Some(scripted.connected().await);
    }
    let side = bob.as_mut().unwrap();
    queued(side, &filename).await;
    side
      .send(
        &PeerMessage::UploadDenied(UploadDenied {
          filename: filename.as_str().into(),
          reason: reason.into(),
        })
        .encode(),
      )
      .await;
    let error = running.result().await.unwrap_err();
    assert!(rejected(reason)(&error), "{reason}: {error:?}");
    let message = scripted
      .session
      .event(|event| match event {
        Event::DownloadDenied { message, .. } => Some(message),
        _ => None,
      })
      .await;
    assert_eq!(message, reason);
  }
}

#[tokio::test]
async fn a_transfer_request_answered_with_a_refusal_rejects_the_download_and_queued_waits() {
  let mut scripted = Scripted::start().await;
  let running = scripted.download("@@share\\a.mp3", |request| {
    request.options.request_style = RequestStyle::TransferRequest;
  });
  let mut bob = scripted.connected().await;
  let PeerMessage::TransferRequest(asked) = bob.read_peer().await else {
    panic!("a transfer request");
  };
  assert_eq!(asked.direction, TransferDirection::Download);
  assert_eq!(asked.file_size, None, "a download request carries no size");
  bob
    .send(
      &PeerMessage::TransferResponse(TransferResponse::Denied {
        token: asked.token,
        message: "Too many megabytes".into(),
      })
      .encode(),
    )
    .await;
  assert!(rejected("Too many megabytes")(
    &running.result().await.unwrap_err()
  ));

  // "Queued", however spelled, queues the download, which then waits for the offer
  let running = scripted.download("@@share\\b.mp3", |request| {
    request.options.request_style = RequestStyle::TransferRequest;
  });
  let PeerMessage::TransferRequest(asked) = bob.read_peer().await else {
    panic!("a transfer request");
  };
  bob
    .send(
      &PeerMessage::TransferResponse(TransferResponse::Denied {
        token: asked.token,
        message: "queued.".into(),
      })
      .encode(),
    )
    .await;
  let mut events = scripted.client().subscribe();
  let states = tokio::time::timeout(PATIENCE, async {
    loop {
      let downloads = scripted.client().downloads();
      if downloads[0].state == TransferState::QUEUED_REMOTELY {
        return downloads;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .unwrap();
  assert_eq!(states.len(), 1);
  offer(&mut bob, "@@share\\b.mp3", 77, Some(SIZE)).await;
  let mut transfer = scripted.connect_for_transfer(77).await;
  send_file(&mut transfer, 0, SIZE as usize).await;
  transfer.closed().await;
  let done = running.result().await.unwrap();
  assert_eq!(done.remote_token, Some(77));
  let (_, last) = support::transfers::states(&mut events, TransferDirection::Download).await;
  assert!(last.state.succeeded());
}

#[tokio::test]
async fn a_peer_that_sends_at_once_is_dialed_with_our_own_token() {
  let mut scripted = Scripted::start().await;
  let mut events = scripted.client().subscribe();
  let running = scripted.download("@@share\\now.mp3", |request| {
    request.options.request_style = RequestStyle::TransferRequest;
  });
  let mut bob = scripted.connected().await;
  let PeerMessage::TransferRequest(asked) = bob.read_peer().await else {
    panic!("a transfer request");
  };
  bob
    .send(
      &PeerMessage::TransferResponse(TransferResponse::Allowed {
        token: asked.token,
        file_size: Some(SIZE),
      })
      .encode(),
    )
    .await;
  let (mut transfer, token) = scripted.accept_transfer().await;
  assert_eq!(token, asked.token);
  send_file(&mut transfer, 0, SIZE as usize).await;
  transfer.closed().await;
  let destination = running.destination.clone();
  let done = running.result().await.unwrap();
  assert_eq!(done.remote_token, None);
  assert_eq!(std::fs::read(destination).unwrap(), content(SIZE as usize));
  let (states, _) = states(&mut events, TransferDirection::Download).await;
  assert_eq!(
    states,
    [
      TransferState::QUEUED_LOCALLY,
      TransferState::REQUESTED,
      TransferState::QUEUED_REMOTELY,
      TransferState::INITIALIZING,
      TransferState::IN_PROGRESS,
      TransferState::COMPLETED | TransferState::SUCCEEDED,
    ]
  );
}

#[tokio::test]
async fn a_failure_reported_at_any_stage_ends_the_download_at_once() {
  let failed = |filename: &str| {
    PeerMessage::UploadFailed(UploadFailed {
      filename: filename.into(),
    })
    .encode()
  };

  // waiting in bob's queue
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = scripted.queued_download("@@share\\q.mp3").await;
  bob.send(&failed("@@share\\q.mp3")).await;
  assert!(reported_failed(&running.result().await.unwrap_err()));

  // waiting for bob to connect, which takes far longer to time out than this test waits
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = scripted.queued_download("@@share\\i.mp3").await;
  offer(&mut bob, "@@share\\i.mp3", 5, Some(SIZE)).await;
  bob.send(&failed("@@share\\i.mp3")).await;
  let error = running.result().await.unwrap_err();
  assert!(reported_failed(&error), "{error:?}");

  // with bytes moving
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = scripted.queued_download("@@share\\p.mp3").await;
  offer(&mut bob, "@@share\\p.mp3", 6, Some(SIZE)).await;
  let mut transfer = scripted.connect_for_transfer(6).await;
  send_file(&mut transfer, 0, 1000).await;
  bob.send(&failed("@@share\\p.mp3")).await;
  let destination = running.destination.clone();
  let error = running.result().await.unwrap_err();
  assert!(reported_failed(&error), "{error:?}");
  // what arrived is kept for a resume
  let kept = std::fs::read(destination).unwrap();
  assert_eq!(kept, content(1000));
  let downloads = scripted.client().downloads();
  assert!(downloads.is_empty());
}

#[tokio::test]
async fn a_refusal_while_the_request_awaits_its_answer_ends_the_download_at_once() {
  let mut scripted = Scripted::start().await;
  let running = scripted.download("@@share\\r.mp3", |request| {
    request.options.request_style = RequestStyle::TransferRequest;
  });
  let mut bob = scripted.connected().await;
  let _ = bob.read_peer().await;
  bob
    .send(
      &PeerMessage::UploadDenied(UploadDenied {
        filename: "@@share\\r.mp3".into(),
        reason: "Banned".into(),
      })
      .encode(),
    )
    .await;
  let started = tokio::time::Instant::now();
  assert!(rejected("Banned")(&running.result().await.unwrap_err()));
  assert!(
    started.elapsed() < Duration::from_secs(5),
    "not the response timeout"
  );
}

#[tokio::test]
async fn the_size_the_uploader_names_is_checked_against_ours() {
  // another size aborts
  let mut scripted = Scripted::start().await;
  let mut events = scripted.client().subscribe();
  let (running, mut bob) = scripted.queued_download("@@share\\big.mp3").await;
  bob
    .send(
      &PeerMessage::TransferRequest(TransferRequest {
        direction: TransferDirection::Upload,
        token: 9,
        filename: "@@share\\big.mp3".into(),
        file_size: Some(SIZE + 1),
      })
      .encode(),
    )
    .await;
  let error = running.result().await.unwrap_err();
  assert!(
    matches!(error, ClientError::SizeMismatch { local: SIZE, remote, .. } if remote == SIZE + 1),
    "{error:?}"
  );
  let (_, last) = states(&mut events, TransferDirection::Download).await;
  assert_eq!(
    last.state,
    TransferState::COMPLETED | TransferState::ABORTED
  );

  // no size at all keeps ours
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = scripted.queued_download("@@share\\none.mp3").await;
  offer(&mut bob, "@@share\\none.mp3", 10, None).await;
  let mut transfer = scripted.connect_for_transfer(10).await;
  send_file(&mut transfer, 0, SIZE as usize).await;
  assert_eq!(running.result().await.unwrap().size, SIZE);

  // a resolver can settle on the uploader's size
  let mut scripted = Scripted::start().await;
  let running = scripted.download("@@share\\settled.mp3", |request| {
    request.size = SIZE - 10;
    request.options.size_mismatch_resolver = Some(Arc::new(|_, remote| Ok(remote)));
  });
  let mut bob = scripted.connected().await;
  queued(&mut bob, "@@share\\settled.mp3").await;
  offer(&mut bob, "@@share\\settled.mp3", 11, Some(SIZE)).await;
  let mut transfer = scripted.connect_for_transfer(11).await;
  send_file(&mut transfer, 0, SIZE as usize).await;
  let done = running.result().await.unwrap();
  assert_eq!((done.size, done.bytes_transferred), (SIZE, SIZE));
}

#[tokio::test]
async fn a_transfer_message_that_does_not_decode_fails_what_waits_for_it() {
  // a refusal whose reason is cut short still names the file
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = scripted.queued_download("@@share\\x.mp3").await;
  let filename = "@@share\\x.mp3";
  let mut payload = (filename.len() as u32).to_le_bytes().to_vec();
  payload.extend(filename.as_bytes());
  payload.extend([100, 0, 0, 0, b'n', b'o']);
  let mut frame = ((payload.len() + 4) as u32).to_le_bytes().to_vec();
  frame.extend(50u32.to_le_bytes());
  frame.extend(payload);
  bob.send(&frame).await;
  let error = running.result().await.unwrap_err();
  assert!(
    matches!(&error, ClientError::Message { message, .. } if message == "peer sent an unparseable UploadDenied"),
    "{error:?}"
  );
}

#[tokio::test]
async fn a_governor_that_allows_nothing_yet_is_asked_again() {
  let mut scripted = Scripted::start().await;
  let asked = Arc::new(AtomicU32::new(0));
  let reported = Arc::new(AtomicU32::new(0));
  let running = scripted.download("@@share\\paced.mp3", |request| {
    let asked = asked.clone();
    request.options.governor = Some(Arc::new(move |_, wanted, _| {
      // nothing the first time
      let first = asked.fetch_add(1, Ordering::SeqCst) == 0;
      Box::pin(async move { if first { 0 } else { wanted } })
    }));
    let reported = reported.clone();
    request.options.reporter = Some(Arc::new(move |_, _, _, moved| {
      reported.fetch_add(moved, Ordering::SeqCst);
    }));
  });
  let mut bob = scripted.connected().await;
  queued(&mut bob, "@@share\\paced.mp3").await;
  offer(&mut bob, "@@share\\paced.mp3", 19, Some(SIZE)).await;
  let mut transfer = scripted.connect_for_transfer(19).await;
  send_file(&mut transfer, 0, SIZE as usize).await;
  assert_eq!(running.result().await.unwrap().bytes_transferred, SIZE);
  assert!(asked.load(Ordering::SeqCst) > 1);
  assert_eq!(u64::from(reported.load(Ordering::SeqCst)), SIZE);
}

#[tokio::test]
async fn a_fresh_token_skips_one_a_transfer_holds() {
  let scripted = Scripted::start().await;
  // the token the counter hands out next is taken by hand first
  let next = scripted.client().next_token() + 1;
  let held = scripted.download("@@share\\held.mp3", |request| {
    request.token = Some(next);
  });
  tokio::time::timeout(PATIENCE, async {
    while scripted.client().downloads().is_empty() {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  let fresh = scripted.download("@@share\\fresh.mp3", |_| {});
  let tokens = tokio::time::timeout(PATIENCE, async {
    loop {
      let mut tokens: Vec<_> = scripted
        .client()
        .downloads()
        .iter()
        .map(|transfer| transfer.token)
        .collect();
      if tokens.len() == 2 {
        tokens.sort_unstable();
        return tokens;
      }
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  assert_eq!(tokens, [next, next + 1]);
  held.cancel.cancel();
  fresh.cancel.cancel();
}

#[tokio::test]
async fn one_download_per_file_and_one_transfer_per_token() {
  let mut scripted = Scripted::start().await;
  let (_running, _bob) = scripted.queued_download("@@share\\once.mp3").await;
  let again = scripted
    .client()
    .enqueue_download(
      DownloadRequest::new(
        BOB,
        "@@share\\once.mp3",
        SIZE,
        DownloadOutput::File(scratch().join("again")),
      ),
      &CancellationToken::new(),
    )
    .await;
  assert!(matches!(again, Err(ClientError::DuplicateTransfer { .. })));
  let token = scripted.client().downloads()[0].token;
  let mut request = DownloadRequest::new(
    BOB,
    "@@share\\other.mp3",
    SIZE,
    DownloadOutput::File(scratch().join("other")),
  );
  request.token = Some(token);
  assert!(matches!(
    scripted
      .client()
      .enqueue_download(request, &CancellationToken::new())
      .await,
    Err(ClientError::DuplicateToken(taken)) if taken == token
  ));
}
