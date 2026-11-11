//! Files a user sends unasked: between two clients on soulfind, the receiver's application is
//! asked, and either starts a download that takes the held offer or refuses it; from a scripted
//! peer, an offer of a file whose offer is held is refused, and an offer nobody takes is refused
//! once its time is up.

use std::sync::Arc;

use std::time::Duration;

use crate::support::peer::PeerSide;
use crate::support::scripted::{self, BOB, Scripted, send_file};
use crate::support::soulfind::{self, User};
use crate::support::transfers::{content, file, scratch};
use crate::support::{PATIENCE, listening_options};
use soulseek_client::{
  ClientError, ClientOptions, DownloadOutput, DownloadRequest, RequestStyle, UploadInput,
  UploadRequest,
};
use soulseek_proto::peer::{TransferRequest, TransferResponse};
use soulseek_proto::{PeerMessage, RemoteFilename, TransferDirection};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

const SIZE: usize = 200_000;
const REMOTE: &str = "@@share\\gift\\01 song.flac";

/// An offer the receiver's application was asked about
type Offer = (String, RemoteFilename, u64);

/// Has `options` take every file offered unasked, handing each offer to the returned receiver,
/// which is where the test starts the download
fn taking(options: &mut ClientOptions) -> mpsc::UnboundedReceiver<Offer> {
  let (offered, offers) = mpsc::unbounded_channel();
  options.resolvers.receive_offer = Some(Arc::new(move |username, _, filename, size| {
    // the test may have stopped listening; the offer is taken either way
    let _ = offered.send((username, filename, size));
    Box::pin(async { Ok(()) })
  }));
  offers
}

/// sends the file at `path` to `receiver` unasked, as `sender`'s upload
fn send(
  sender: &User,
  receiver: &User,
  path: std::path::PathBuf,
) -> tokio::task::JoinHandle<Result<soulseek_client::Transfer, ClientError>> {
  let client = sender.client.clone();
  let request = UploadRequest::new(&receiver.username, REMOTE, UploadInput::File(path));
  tokio::spawn(async move { client.upload(request, &CancellationToken::new()).await })
}

#[tokio::test]
async fn a_file_sent_unasked_lands_once_the_application_takes_it() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let mut options = soulfind::listening();
  let mut offers = taking(&mut options);
  let receiver = server.login("receiver", options).await;
  let sender = server.login("sender", soulfind::listening()).await;
  let dir = scratch();
  let upload = send(&sender, &receiver, file(&dir, "source.flac", SIZE));
  let (username, filename, size) = tokio::time::timeout(PATIENCE, offers.recv())
    .await
    .expect("the receiver is asked about the file")
    .unwrap();
  assert_eq!(
    (username.as_str(), filename.as_str(), size),
    (sender.username.as_str(), REMOTE, SIZE as u64)
  );
  let destination = dir.join("received.flac");
  let mut request = DownloadRequest::new(
    &username,
    filename,
    size,
    DownloadOutput::File(destination.clone()),
  );
  request.options.request_style = RequestStyle::Offered;
  let received = receiver
    .client
    .download(request, &CancellationToken::new())
    .await
    .unwrap();
  assert_eq!(received.bytes_transferred, SIZE as u64);
  assert_eq!(std::fs::read(destination).unwrap(), content(SIZE));
  upload.await.unwrap().expect("the sender's upload succeeds");
}

#[tokio::test]
async fn a_file_the_application_refuses_is_refused_to_its_sender() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let mut options = soulfind::listening();
  options.resolvers.receive_offer = Some(Arc::new(|_, _, _, _| {
    Box::pin(async { Err(ClientError::TransferRejected("not a buddy".into())) })
  }));
  let receiver = server.login("receiver", options).await;
  let sender = server.login("sender", soulfind::listening()).await;
  let dir = scratch();
  let refused = send(&sender, &receiver, file(&dir, "source.flac", SIZE))
    .await
    .unwrap()
    .unwrap_err();
  assert!(
    matches!(&refused, ClientError::TransferRejected(reason) if reason == "Cancelled"),
    "{refused:?}"
  );
  // the refused offer is not held, so a download cannot take it afterwards
  let mut request = DownloadRequest::new(
    &sender.username,
    REMOTE,
    SIZE as u64,
    DownloadOutput::File(dir.join("received.flac")),
  );
  request.options.request_style = RequestStyle::Offered;
  let error = receiver
    .client
    .download(request, &CancellationToken::new())
    .await
    .unwrap_err();
  assert!(
    matches!(error, ClientError::InvalidOperation(_)),
    "{error:?}"
  );
}

/// bob offers to send `filename` of `size` bytes unasked, with `token`
async fn bob_offers(bob: &mut PeerSide, filename: &str, token: u32, size: u64) {
  let offer = PeerMessage::TransferRequest(TransferRequest {
    direction: TransferDirection::Upload,
    token,
    filename: filename.into(),
    file_size: Some(size),
  });
  bob.send(&offer.encode()).await;
}

/// the client's refusal of bob's offer with `token`
fn refused(token: u32) -> PeerMessage {
  PeerMessage::TransferResponse(TransferResponse::Denied {
    token,
    message: "Cancelled".into(),
  })
}

#[tokio::test]
async fn an_offer_of_a_file_whose_offer_is_held_is_refused() {
  const FILE: &str = "@@share\\gift.flac";
  let mut options = listening_options();
  let (asked, mut asks) = mpsc::unbounded_channel();
  let decide = Arc::new(Notify::new());
  let decided = decide.clone();
  options.resolvers.receive_offer = Some(Arc::new(move |_, _, _, size| {
    let decided = decided.clone();
    // the test may have stopped listening; the offer is decided either way
    let _ = asked.send(size);
    Box::pin(async move {
      decided.notified().await;
      Ok(())
    })
  }));
  let mut scripted = Scripted::with_options(options).await;
  let mut bob = scripted.session.peer(BOB).await;
  bob_offers(&mut bob, FILE, 1, scripted::SIZE).await;
  let size = tokio::time::timeout(PATIENCE, asks.recv()).await.unwrap();
  assert_eq!(size, Some(scripted::SIZE));
  // the same file again, far larger, while the first offer is being decided
  bob_offers(&mut bob, FILE, 2, scripted::SIZE * 1000).await;
  assert_eq!(bob.read_peer().await, refused(2));
  decide.notify_one();
  let running = scripted.download(FILE, |request| {
    request.options.request_style = RequestStyle::Offered;
  });
  // the download looks bob up, then answers on the connection bob offered on
  scripted.give_address().await;
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::TransferResponse(TransferResponse::Allowed {
      token: 1,
      file_size: None,
    })
  );
  let mut transfer = scripted.connect_for_transfer(1).await;
  send_file(&mut transfer, 0, scripted::SIZE as usize).await;
  let done = running.result().await.unwrap();
  assert_eq!(
    (done.remote_token, done.bytes_transferred),
    (Some(1), scripted::SIZE)
  );
  assert!(
    asks.try_recv().is_err(),
    "the second offer is never asked about"
  );
}

#[tokio::test]
async fn an_offer_nobody_takes_is_refused_once_its_time_is_up() {
  let mut options = listening_options();
  options.unasked_offer_timeout = Duration::from_millis(200);
  options.resolvers.receive_offer = Some(Arc::new(|_, _, _, _| Box::pin(async { Ok(()) })));
  let scripted = Scripted::with_options(options).await;
  let mut bob = scripted.session.peer(BOB).await;
  bob_offers(&mut bob, "@@share\\untaken.flac", 7, scripted::SIZE).await;
  let answer = tokio::time::timeout(PATIENCE, bob.read_peer())
    .await
    .expect("the offer is answered");
  assert_eq!(answer, refused(7));
}
