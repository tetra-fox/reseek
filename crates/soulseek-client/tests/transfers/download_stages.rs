//! Downloads from a scripted peer that drops its connections, never connects back, stops
//! sending or lies about the file, and downloads canceled at each stage.

use std::time::Duration;

use crate::support::scripted::{BOB, SIZE, Scripted, offer, queued, send_file};
use crate::support::transfers::{content, reached, states};
use crate::support::{listening_options, next_event};
use soulseek_client::{ClientError, Event, RequestStyle, Transfer, TransferState};
use soulseek_proto::TransferDirection;

#[tokio::test]
async fn a_queued_download_outlives_the_message_connection() {
  let mut scripted = Scripted::start().await;
  let (running, bob) = scripted.queued_download("@@share\\long.mp3").await;
  // bob's connection closes while the download waits; bob comes back on a new one later
  drop(bob);
  tokio::time::sleep(Duration::from_millis(200)).await;
  assert_eq!(
    scripted.client().downloads()[0].state,
    TransferState::QUEUED_REMOTELY
  );
  let mut bob = scripted.session.peer(BOB).await;
  offer(&mut bob, "@@share\\long.mp3", 12, Some(SIZE)).await;
  let mut transfer = scripted.connect_for_transfer(12).await;
  send_file(&mut transfer, 0, SIZE as usize).await;
  assert_eq!(running.result().await.unwrap().bytes_transferred, SIZE);
}

#[tokio::test]
async fn an_uploader_that_never_connects_gets_a_second_chance_from_our_side() {
  let mut options = listening_options();
  options.transfer_connection_options.connect_timeout = Duration::from_millis(300);
  let mut scripted = Scripted::with_options(options).await;
  let (running, mut bob) = scripted.queued_download("@@share\\back.mp3").await;
  offer(&mut bob, "@@share\\back.mp3", 13, Some(SIZE)).await;
  // bob does not connect; after the wait the client connects to bob with bob's token
  let (mut transfer, token) = scripted.accept_transfer().await;
  assert_eq!(token, 13);
  send_file(&mut transfer, 0, SIZE as usize).await;
  transfer.closed().await;
  assert_eq!(running.result().await.unwrap().bytes_transferred, SIZE);
}

#[tokio::test]
async fn an_uploader_that_closes_early_fails_the_download_and_the_bytes_stay() {
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = scripted.queued_download("@@share\\short.mp3").await;
  offer(&mut bob, "@@share\\short.mp3", 14, Some(SIZE)).await;
  let mut transfer = scripted.connect_for_transfer(14).await;
  // claims the full size, sends a part and closes
  send_file(&mut transfer, 0, 20_000).await;
  drop(transfer);
  let destination = running.destination.clone();
  let error = running.result().await.unwrap_err();
  assert!(
    matches!(&error, ClientError::Connection { message, .. } if message.starts_with("Transfer failed")),
    "{error:?}"
  );
  assert_eq!(std::fs::read(destination).unwrap(), content(20_000));
}

#[tokio::test]
async fn waits_that_take_too_long_time_the_download_out() {
  // the answer to a transfer request
  let mut options = listening_options();
  options.transfer_response_timeout = Duration::from_millis(200);
  let mut scripted = Scripted::with_options(options).await;
  let mut events = scripted.client().subscribe();
  let running = scripted.download("@@share\\slow.mp3", |request| {
    request.options.request_style = RequestStyle::TransferRequest;
  });
  let _bob = scripted.connected().await;
  assert!(matches!(
    running.result().await.unwrap_err(),
    ClientError::Timeout(_)
  ));
  let (_, last) = states(&mut events, TransferDirection::Download).await;
  assert_eq!(
    last.state,
    TransferState::COMPLETED | TransferState::TIMED_OUT
  );

  // the wait in the queue, when the caller bounds it
  let mut scripted = Scripted::start().await;
  let running = scripted.download("@@share\\never.mp3", |request| {
    request.options.queued_timeout = Some(Duration::from_millis(200));
  });
  let mut bob = scripted.connected().await;
  queued(&mut bob, "@@share\\never.mp3").await;
  assert!(matches!(
    running.result().await.unwrap_err(),
    ClientError::Timeout(_)
  ));

  // bytes stop coming
  let mut options = listening_options();
  options.transfer_connection_options.inactivity_timeout = Some(Duration::from_millis(300));
  let mut scripted = Scripted::with_options(options).await;
  let (running, mut bob) = scripted.queued_download("@@share\\stall.mp3").await;
  offer(&mut bob, "@@share\\stall.mp3", 15, Some(SIZE)).await;
  let mut transfer = scripted.connect_for_transfer(15).await;
  send_file(&mut transfer, 0, 100).await;
  assert!(matches!(
    running.result().await.unwrap_err(),
    ClientError::Timeout(_)
  ));
}

#[tokio::test]
async fn a_download_canceled_at_any_stage_ends_canceled() {
  let canceled =
    |result: Result<Transfer, ClientError>| matches!(result, Err(ClientError::Canceled));

  // waiting for a download slot
  let mut options = listening_options();
  options.maximum_concurrent_downloads = 1;
  let mut scripted = Scripted::with_options(options).await;
  let mut events = scripted.client().subscribe();
  let (_first, _bob) = scripted.queued_download("@@share\\first.mp3").await;
  // the first download's states
  let _ = next_event(&mut events, |event| match event {
    Event::TransferStateChanged { transfer, .. }
      if transfer.state == TransferState::QUEUED_REMOTELY =>
    {
      Some(())
    }
    _ => None,
  })
  .await;
  let second = scripted.download("@@share\\second.mp3", |_| {});
  let (state, _) = next_event(&mut events, |event| match event {
    Event::TransferStateChanged { transfer, .. } => Some((transfer.state, transfer.filename)),
    _ => None,
  })
  .await;
  assert_eq!(state, TransferState::QUEUED_LOCALLY);
  second.cancel.cancel();
  assert!(canceled(second.task.await.unwrap()));

  // waiting for the answer to a transfer request
  let mut scripted = Scripted::start().await;
  let running = scripted.download("@@share\\asked.mp3", |request| {
    request.options.request_style = RequestStyle::TransferRequest;
  });
  let mut bob = scripted.connected().await;
  let _ = bob.read_peer().await;
  running.cancel.cancel();
  assert!(canceled(running.result().await));

  // waiting for bob to connect: bob connecting afterwards is turned away
  let mut scripted = Scripted::start().await;
  let (running, mut bob) = scripted.queued_download("@@share\\init.mp3").await;
  offer(&mut bob, "@@share\\init.mp3", 17, Some(SIZE)).await;
  running.cancel.cancel();
  assert!(canceled(running.result().await));
  let mut late = scripted.connect_for_transfer(17).await;
  late.closed().await;

  // with bytes moving: the connection closes and what arrived stays
  let mut scripted = Scripted::start().await;
  let mut events = scripted.client().subscribe();
  let (running, mut bob) = scripted.queued_download("@@share\\moving.mp3").await;
  offer(&mut bob, "@@share\\moving.mp3", 18, Some(SIZE)).await;
  let mut transfer = scripted.connect_for_transfer(18).await;
  send_file(&mut transfer, 0, 3000).await;
  reached(&mut events, 3000).await;
  running.cancel.cancel();
  let destination = running.destination.clone();
  assert!(canceled(running.result().await));
  transfer.closed().await;
  assert_eq!(std::fs::read(destination).unwrap(), content(3000));
}

#[tokio::test]
async fn disposing_the_client_cancels_its_transfers() {
  let mut scripted = Scripted::start().await;
  let (running, _bob) = scripted.queued_download("@@share\\disposed.mp3").await;
  scripted.client().dispose().await;
  assert!(matches!(running.result().await, Err(ClientError::Canceled)));
}
