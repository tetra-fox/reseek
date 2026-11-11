//! Files a user sends the daemon unasked, over soulfind: refused by default, refused from a user
//! who is not a buddy when only buddies may send, refused in a buddy's name from another address
//! than the server reports for them, taken from the buddy and landing like any other download,
//! and refused for a file whose download from them was just canceled. From a scripted sender:
//! such a file never replaces one, and a retry is held to the size first offered.

mod support;

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use api_client::Client;
use api_types::options::OptionsPatch;
use api_types::transfers::{Transfer, TransferDirection, TransferStates};
use api_types::users::BuddyChange;
use serde_json::json;
use soulseek_client::{ClientError, ClientOptions, UploadInput, UploadRequest};
use soulseek_proto::init::PeerInit;
use soulseek_proto::peer::{QueueDownload, TransferRequest, TransferResponse, UploadFailed};
use soulseek_proto::{InitMessage, PeerConnectionType, PeerMessage};
use support::peer::PeerSide;
use support::scripted::{Scripted, send_from};
use support::soulfind::{self, Soulfind, User};
use support::transfers::{self, Asked, asked, content, file, scratch};
use support::{
  PATIENCE, completed, daemon_listening, download_when, enqueue, free_port, logged_in, signed_in,
  until,
};
use tokio_util::sync::CancellationToken;

const SIZE: usize = 200_000;
const GIFT: &str = "@@share\\gift\\01 song.flac";
const TAKEN_BACK: &str = "@@share\\gift\\02 song.flac";

/// A sender that queues every request it is asked, so a download from it waits there.
async fn sender(server: &Arc<Soulfind>) -> (User, Asked) {
  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  let requests = transfers::queueing(&mut options);
  (server.login("sender", options).await, requests)
}

/// What sending a file unasked came to, on the sender's side.
type Sent = Result<soulseek_client::Transfer, ClientError>;

/// Sends `remote` to the daemon `to` unasked, as `sender`'s upload.
async fn send(sender: &User, to: &str, remote: &str) -> Sent {
  let source = file(&scratch(), "gift.flac", SIZE);
  let request = UploadRequest::new(to, remote, UploadInput::File(source));
  sender
    .client
    .upload(request, &CancellationToken::new())
    .await
}

/// Whether a send was refused the way the daemon refuses a file it does not take.
fn refused(sent: &Sent) -> bool {
  matches!(sent, Err(ClientError::TransferRejected(reason)) if reason == "Cancelled")
}

/// Lets the users in `groups` send the daemon files.
async fn let_send(api: &Client, groups: serde_json::Value) {
  let patch = OptionsPatch {
    set: BTreeMap::from([("transfers.download.received.from".to_string(), groups)]),
    unset: Vec::new(),
    expires_in: None,
    revision: None,
  };
  api.patch_options(&patch).await.unwrap();
}

/// Offers `remote` of `size` bytes to the daemon as `username` on `side`, under `token`.
async fn offer(side: &mut PeerSide, remote: &str, token: u32, size: u64) {
  let offer = PeerMessage::TransferRequest(TransferRequest {
    direction: soulseek_proto::TransferDirection::Upload,
    token,
    filename: remote.into(),
    file_size: Some(size),
  });
  side.send(&offer.encode()).await;
}

/// A message connection to the daemon listening on `listen`, introduced as `username`, from
/// `from`; `None` when this machine cannot send from that address.
async fn introduced(listen: u16, username: &str, from: Ipv4Addr) -> Option<PeerSide> {
  let socket = tokio::net::TcpSocket::new_v4().unwrap();
  socket.bind(SocketAddr::from((from, 0))).ok()?;
  let stream = socket
    .connect(SocketAddr::from((Ipv4Addr::LOCALHOST, listen)))
    .await
    .unwrap();
  let mut side = PeerSide::new(stream);
  let hello = InitMessage::PeerInit(PeerInit {
    username: username.into(),
    connection_type: PeerConnectionType::Peer,
    token: 0,
  });
  side.send(&hello.encode()).await;
  Some(side)
}

/// The download of `remote` from `username` once `ready` holds for it.
async fn download_of(
  api: &Client,
  username: &str,
  remote: &str,
  what: &str,
  ready: impl Fn(&Transfer) -> bool,
) -> Transfer {
  let started = Instant::now();
  loop {
    let found = api
      .downloads(false)
      .await
      .unwrap()
      .into_iter()
      .filter(|user| user.username == username)
      .flat_map(|user| user.directories)
      .flat_map(|directory| directory.files)
      .find(|t| t.filename == remote && ready(t));
    if let Some(transfer) = found {
      return transfer;
    }
    assert!(started.elapsed() < PATIENCE, "the file sent never {what}");
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
}

fn moved(transfer: &Transfer) -> bool {
  transfer.local_path.is_some()
}

#[tokio::test]
async fn a_file_sent_unasked_is_taken_only_from_a_buddy_on_their_own_address() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (sender, mut requests) = sender(&server).await;
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let listen = free_port();
  let daemon = daemon_listening(dir.path(), &server, &name, listen, &[]);
  let api = signed_in(&daemon).await;
  until(&api, "logged in", logged_in).await;

  assert!(
    refused(&send(&sender, &name, GIFT).await),
    "no one may send files by default"
  );
  let_send(&api, json!(["buddies"])).await;
  assert!(
    refused(&send(&sender, &name, GIFT).await),
    "only buddies may send"
  );
  api
    .set_buddy(&sender.username, &BuddyChange::default())
    .await
    .unwrap();
  // macOS answers on 127.0.0.1 alone unless given an alias, so there this check is skipped
  if let Some(mut impostor) =
    introduced(listen, &sender.username, Ipv4Addr::new(127, 0, 0, 2)).await
  {
    offer(&mut impostor, GIFT, 9, SIZE as u64).await;
    assert_eq!(
      impostor.read_peer().await,
      PeerMessage::TransferResponse(TransferResponse::Denied {
        token: 9,
        message: "Cancelled".into(),
      }),
      "a buddy's name from another address is not the buddy"
    );
  }

  send(&sender, &name, GIFT)
    .await
    .expect("the daemon takes the buddy's file");
  let received = download_of(&api, &sender.username, GIFT, "landed", moved).await;
  assert_eq!(received.state, completed(TransferStates::Succeeded));
  let path = dir
    .path()
    .join("data")
    .join("downloads")
    .join("gift")
    .join("01 song.flac");
  assert_eq!(received.local_path.as_deref(), Some(path.to_str().unwrap()));
  assert_eq!(std::fs::read(&path).unwrap(), content(SIZE));
  assert!(received.job_id.is_some(), "the file is a job of its own");

  // a download asked for, then canceled: its sender sending it anyway is them catching up
  let id = enqueue(&api, &sender.username, TAKEN_BACK, SIZE as u64).await;
  let (_, asked_for) = asked(&mut requests).await;
  assert_eq!(asked_for.as_str(), TAKEN_BACK);
  api
    .cancel_transfer(TransferDirection::Download, &sender.username, id, false)
    .await
    .unwrap();
  download_when(&api, &sender.username, id, "canceled", |t| {
    t.state == completed(TransferStates::Cancelled)
  })
  .await;
  assert!(
    refused(&send(&sender, &name, TAKEN_BACK).await),
    "a file whose download was just canceled"
  );
}

#[tokio::test]
async fn a_file_sent_unasked_never_replaces_a_file_and_keeps_its_size() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let config = "[transfers.download.received]\nfrom = [\"default\"]\n\
                [transfers.download.retry]\nattempts = 2\ndelay = \"1s\"\n\
                [transfers.download.destination]\nexists = \"overwrite\"\n";
  std::fs::write(dir.path().join("config.toml"), config).unwrap();
  let listen = free_port();
  let daemon = daemon_listening(
    dir.path(),
    &server,
    &soulfind::username("daemon"),
    listen,
    &[],
  );
  let api = signed_in(&daemon).await;
  until(&api, "logged in", logged_in).await;
  let sender = Scripted::start(&server).await;
  let mut side = introduced(listen, sender.name(), Ipv4Addr::LOCALHOST)
    .await
    .unwrap();
  let allowed = |token| {
    PeerMessage::TransferResponse(TransferResponse::Allowed {
      token,
      file_size: None,
    })
  };

  // a file of ours where the sender names theirs: the options would overwrite it, theirs not
  let ours = dir.path().join("data").join("downloads").join("gift");
  std::fs::create_dir_all(&ours).unwrap();
  let ours = ours.join("01 song.flac");
  std::fs::write(&ours, b"ours").unwrap();
  offer(&mut side, GIFT, 1, 100).await;
  assert_eq!(side.read_peer().await, allowed(1));
  send_from(
    SocketAddr::from((Ipv4Addr::LOCALHOST, listen)),
    sender.name(),
    1,
    0,
    100,
  )
  .await;
  let theirs = download_of(&api, sender.name(), GIFT, "landed", moved).await;
  let landed = theirs.local_path.unwrap();
  assert_ne!(landed, ours.to_str().unwrap(), "renamed");
  assert_eq!(std::fs::read(&ours).unwrap(), b"ours");
  assert_eq!(std::fs::read(&landed).unwrap(), content(100));

  // the first try fails; the retry asks for the file, and the sender names a larger size
  offer(&mut side, TAKEN_BACK, 2, 100).await;
  assert_eq!(side.read_peer().await, allowed(2));
  let failed = PeerMessage::UploadFailed(UploadFailed {
    filename: TAKEN_BACK.into(),
  });
  side.send(&failed.encode()).await;
  assert_eq!(
    side.read_peer().await,
    PeerMessage::QueueDownload(QueueDownload {
      filename: TAKEN_BACK.into(),
    })
  );
  offer(&mut side, TAKEN_BACK, 3, 200).await;
  let disputed = download_of(&api, sender.name(), TAKEN_BACK, "ended", |t| {
    t.ended_at.is_some()
  })
  .await;
  assert_eq!(disputed.state, completed(TransferStates::Aborted));
  assert_eq!(
    disputed.exception.as_deref(),
    Some("the remote size of 200 does not match expected size 100")
  );
}
