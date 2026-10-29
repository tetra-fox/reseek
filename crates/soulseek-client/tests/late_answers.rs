//! A scripted peer answering, late, the connection request the server relayed for us: dialling
//! it directly already won, but the peer connects to us anyway with the request's token. For
//! messages, the answering connection replaces the one we dialled, and the one it replaced is
//! still read; for a transfer it is refused, since the transfer runs on the connection that won;
//! after the time the request was awaited for, it is unknown.

mod support;

use std::time::Duration;

use soulseek_client::{ClientOptions, RequestStyle};
use soulseek_proto::init::PierceFirewall;
use soulseek_proto::peer::{TransferResponse, UserInfo};
use soulseek_proto::server::ConnectToPeerRequest;
use soulseek_proto::{InitMessage, PeerConnectionType, PeerMessage, ServerRequest};
use support::peer::PeerSide;
use support::scripted::{BOB, SIZE, Scripted, offer, queued, send_file};
use support::{Logs, PATIENCE, listening_options};

/// the token of the next request the client has the server relay to bob for `connection_type`
async fn relayed(scripted: &mut Scripted, connection_type: PeerConnectionType) -> u32 {
  scripted
    .session
    .server
    .request_matching(|request| match request {
      ServerRequest::ConnectToPeer(ConnectToPeerRequest {
        token,
        username,
        connection_type: asked,
      }) if username == BOB && asked == connection_type => Some(token),
      _ => None,
    })
    .await
}

/// bob connects to the client and answers the relayed request `token`
async fn answer(scripted: &Scripted, token: u32) -> PeerSide {
  let mut side = PeerSide::connect(scripted.session.listen_address()).await;
  side
    .send(&InitMessage::PierceFirewall(PierceFirewall { token }).encode())
    .await;
  side
}

/// waits until the client's message connection to bob is the one bob holds as `side`
async fn current(scripted: &Scripted, side: &PeerSide) {
  let local = side.stream.local_addr().unwrap();
  tokio::time::timeout(PATIENCE, async {
    while scripted.client().message_connections() != [(BOB.to_owned(), local)] {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("the connection becomes the current one");
}

#[tokio::test]
async fn a_late_answer_replaces_the_dialled_connection_and_carries_the_offer() {
  let mut scripted = Scripted::start().await;
  let running = scripted.download("@@share\\late.mp3", |_| {});
  let mut dialled = scripted.connected().await;
  queued(&mut dialled, "@@share\\late.mp3").await;
  // bob acts on the relayed request after all, counts that connection as its connection to us
  // and drops the one we dialled
  let token = relayed(&mut scripted, PeerConnectionType::Peer).await;
  let mut answered = answer(&scripted, token).await;
  current(&scripted, &answered).await;
  drop(dialled);
  offer(&mut answered, "@@share\\late.mp3", 77, Some(SIZE)).await;
  let mut transfer = scripted.connect_for_transfer(77).await;
  send_file(&mut transfer, 0, SIZE as usize).await;
  transfer.closed().await;
  let done = running.result().await.unwrap();
  assert_eq!(done.remote_token, Some(77));
  current(&scripted, &answered).await;
}

#[tokio::test]
async fn the_replaced_connection_is_still_read_and_answered() {
  let mut scripted = Scripted::start().await;
  let _running = scripted.download("@@share\\late.mp3", |_| {});
  let mut dialled = scripted.connected().await;
  queued(&mut dialled, "@@share\\late.mp3").await;
  let token = relayed(&mut scripted, PeerConnectionType::Peer).await;
  let answered = answer(&scripted, token).await;
  current(&scripted, &answered).await;
  // what bob sent on the first connection before it moved over still reaches the client
  dialled.send(&PeerMessage::InfoRequest.encode()).await;
  let PeerMessage::InfoResponse(UserInfo { .. }) = dialled.read_peer().await else {
    panic!("expected user info on the replaced connection");
  };
  current(&scripted, &answered).await;
}

#[tokio::test]
async fn an_answer_after_the_request_was_awaited_for_is_refused() {
  let logs = Logs::capture();
  let mut options: ClientOptions = listening_options();
  options.peer_connection_options.connect_timeout = Duration::from_millis(300);
  let mut scripted = Scripted::with_options(options).await;
  let _running = scripted.download("@@share\\late.mp3", |_| {});
  let mut dialled = scripted.connected().await;
  queued(&mut dialled, "@@share\\late.mp3").await;
  let token = relayed(&mut scripted, PeerConnectionType::Peer).await;
  // the request would have been given up by now even if dialling had failed
  tokio::time::sleep(Duration::from_millis(400)).await;
  let mut answered = answer(&scripted, token).await;
  answered.closed().await;
  logs
    .wait_for(&[
      "connection closed",
      &format!("Unknown PierceFirewall attempt with token {token}"),
    ])
    .await;
  current(&scripted, &dialled).await;
}

#[tokio::test]
async fn a_late_answer_to_a_transfer_request_is_refused() {
  let logs = Logs::capture();
  let mut scripted = Scripted::start().await;
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
  let (mut transfer, _) = scripted.accept_transfer().await;
  let token = relayed(&mut scripted, PeerConnectionType::Transfer).await;
  let mut answered = answer(&scripted, token).await;
  answered.closed().await;
  logs
    .wait_for(&[
      "connection closed",
      "Transfer connection request already answered",
    ])
    .await;
  // the transfer runs on the connection that won, and the message connection stays as it was
  send_file(&mut transfer, 0, SIZE as usize).await;
  transfer.closed().await;
  running.result().await.unwrap();
  current(&scripted, &bob).await;
}
