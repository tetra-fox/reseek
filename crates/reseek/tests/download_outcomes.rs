//! Downloads by the daemon from scripted peers logged in to soulfind, for what a real client
//! does not do on cue: a refusal, a failure tried again after its wait, a size that differs
//! with and without a partial file, a resumed partial, a partial as long as the file, a peer
//! that never answers the request or the question of its queue, and the place in its queue a
//! peer gives when asked or on its own.

mod support;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use api_client::Client;
use api_types::events::EventsQuery;
use api_types::transfers::{QueueDownloadRequest, Transfer, TransferDirection, TransferStates};
use serde_json::json;
use soulseek_client::ClientOptions;
use soulseek_proto::init::{FileTransferInit, PeerInit};
use soulseek_proto::peer::{
  PlaceInQueueRequest, PlaceInQueueResponse, QueueDownload, TransferRequest, TransferResponse,
  UploadDenied, UploadFailed,
};
use soulseek_proto::{InitMessage, PeerConnectionType, PeerMessage, TransferDirection as Wire};
use support::peer::PeerSide;
use support::soulfind::{self, Soulfind, User};
use support::transfers::content;
use support::{PATIENCE, daemon_listening, free_port, logged_in, signed_in, until};
use tokio::net::TcpListener;
use uuid::Uuid;

/// A user on soulfind whose peer side the test scripts: soulfind gives out the port of a
/// listener of the test's as the user's own.
struct Scripted {
  user: User,
  listener: TcpListener,
}

impl Scripted {
  async fn start(server: &Arc<Soulfind>) -> Self {
    // soulfind gives out a loopback user's address as the host's LAN address
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    let options = ClientOptions {
      enable_listener: false,
      listen_port: listener.local_addr().unwrap().port(),
      ..ClientOptions::default()
    };
    Self {
      user: server.login("peer", options).await,
      listener,
    }
  }

  fn name(&self) -> &str {
    &self.user.username
  }

  /// The daemon's next message connection to the peer, past its introduction.
  async fn connected(&self) -> PeerSide {
    let (stream, _) = tokio::time::timeout(PATIENCE, self.listener.accept())
      .await
      .expect("the daemon connects")
      .unwrap();
    let mut side = PeerSide::new(stream);
    let InitMessage::PeerInit(PeerInit {
      connection_type, ..
    }) = side.read_init().await
    else {
      panic!("the daemon introduces itself");
    };
    assert_eq!(connection_type, PeerConnectionType::Peer);
    side
  }
}

/// Reads the daemon's request for `filename`.
async fn queued(side: &mut PeerSide, filename: &str) {
  assert_eq!(
    side.read_peer().await,
    PeerMessage::QueueDownload(QueueDownload {
      filename: filename.into(),
    })
  );
}

/// Offers `filename` of `size` bytes under `token` and reads the daemon's acceptance.
async fn offer(side: &mut PeerSide, filename: &str, token: u32, size: u64) {
  let request = PeerMessage::TransferRequest(TransferRequest {
    direction: Wire::Upload,
    token,
    filename: filename.into(),
    file_size: Some(size),
  });
  side.send(&request.encode()).await;
  assert_eq!(
    side.read_peer().await,
    PeerMessage::TransferResponse(TransferResponse::Allowed {
      token,
      file_size: None,
    })
  );
}

/// Connects to the daemon as `username` for the transfer `token`, reads where it asks to
/// start, checks it is `start`, and sends the file of `size` bytes from there.
async fn send_from(daemon: SocketAddr, username: &str, token: u32, start: u64, size: usize) {
  let mut transfer = PeerSide::connect(daemon).await;
  let mut init = InitMessage::PeerInit(PeerInit {
    username: username.into(),
    connection_type: PeerConnectionType::Transfer,
    token: 0,
  })
  .encode();
  init.extend(FileTransferInit { token }.encode());
  transfer.send(&init).await;
  let offset = u64::from_le_bytes(transfer.raw(8).await.try_into().unwrap());
  assert_eq!(offset, start);
  transfer.send(&content(size)[start as usize..]).await;
  transfer.closed().await;
}

/// The download `id` from `username` once `ready` holds for it.
async fn download_when(
  api: &Client,
  username: &str,
  id: Uuid,
  what: &str,
  ready: impl Fn(&Transfer) -> bool,
) -> Transfer {
  let started = Instant::now();
  loop {
    let transfer = api
      .transfer(TransferDirection::Download, username, id, false)
      .await
      .unwrap();
    if ready(&transfer) {
      return transfer;
    }
    assert!(started.elapsed() < PATIENCE, "never {what}: {transfer:?}");
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
}

async fn enqueue(api: &Client, username: &str, filename: &str, size: u64) -> Uuid {
  let files = [QueueDownloadRequest {
    filename: filename.to_string(),
    size,
  }];
  let answer = api.enqueue(username, &files).await.unwrap();
  assert!(answer.failed.is_empty(), "{:?}", answer.failed);
  answer.enqueued[0].id
}

fn ended(transfer: &Transfer) -> bool {
  transfer.ended_at.is_some()
}

fn completed(outcome: TransferStates) -> TransferStates {
  TransferStates::Completed | outcome
}

/// The daemon in `dir` with `config`, logged in to `server`, taking peers' connections on
/// the returned address, and its client.
async fn started(
  dir: &Path,
  server: &Arc<Soulfind>,
  config: &str,
) -> (support::Daemon, Client, SocketAddr) {
  std::fs::write(dir.join("config.toml"), config).unwrap();
  let listen = free_port();
  let daemon = daemon_listening(dir, server, &soulfind::username("daemon"), listen, &[]);
  let api = signed_in(&daemon).await;
  until(&api, "logged in", logged_in).await;
  (daemon, api, SocketAddr::from((Ipv4Addr::LOCALHOST, listen)))
}

#[tokio::test]
async fn refusals_failures_and_sizes_end_each_in_their_outcome() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let config = "[transfers.download.retry]\nattempts = 2\ndelay = \"1s\"\n";
  let (_daemon, api, listen) = started(dir.path(), &server, config).await;

  let refusing = Scripted::start(&server).await;
  let id = enqueue(&api, refusing.name(), "@@share\\a\\no.flac", 10).await;
  let mut side = refusing.connected().await;
  queued(&mut side, "@@share\\a\\no.flac").await;
  let denied = PeerMessage::UploadDenied(UploadDenied {
    filename: "@@share\\a\\no.flac".into(),
    reason: "File not shared.".into(),
  });
  side.send(&denied.encode()).await;
  let rejected = download_when(&api, refusing.name(), id, "rejected", ended).await;
  assert_eq!(rejected.state, completed(TransferStates::Rejected));
  assert_eq!(rejected.exception.as_deref(), Some("File not shared."));
  assert_eq!(rejected.attempts, 1, "a refusal is not tried again");

  let failing = Scripted::start(&server).await;
  let file = "@@share\\a\\fails.flac";
  let id = enqueue(&api, failing.name(), file, 10).await;
  let mut side = failing.connected().await;
  let failed = PeerMessage::UploadFailed(UploadFailed {
    filename: file.into(),
  });
  let place = |place| {
    PeerMessage::PlaceInQueueResponse(PlaceInQueueResponse {
      filename: file.into(),
      place,
    })
    .encode()
  };
  queued(&mut side, file).await;
  download_when(&api, failing.name(), id, "queued", |t| {
    t.state == TransferStates::Queued | TransferStates::Remotely
  })
  .await;
  side.send(&place(5)).await;
  download_when(&api, failing.name(), id, "placed", |t| {
    t.place_in_queue == Some(5)
  })
  .await;
  side.send(&failed.encode()).await;
  let waiting = download_when(&api, failing.name(), id, "waiting to retry", |t| {
    t.attempts == 2
  })
  .await;
  assert_eq!(
    waiting.state,
    TransferStates::Queued | TransferStates::Locally
  );
  assert_eq!(
    waiting.exception.as_deref(),
    Some("Download reported as failed by remote client"),
    "the reason shows while it waits"
  );
  assert_eq!(
    (waiting.started_at, waiting.ended_at, waiting.place_in_queue),
    (None, None, None),
    "the place and the times of the failed try are gone"
  );
  let next = waiting.next_attempt_at.expect("the next try is due");
  let wait = (next - chrono::Utc::now()).num_milliseconds();
  assert!(
    (0..=2000).contains(&wait),
    "a second's delay and up to a second's jitter: {wait}"
  );
  queued(&mut side, file).await;
  download_when(&api, failing.name(), id, "queued", |t| {
    t.state == TransferStates::Queued | TransferStates::Remotely
  })
  .await;
  side.send(&place(4)).await;
  download_when(&api, failing.name(), id, "placed", |t| {
    t.place_in_queue == Some(4)
  })
  .await;
  side.send(&failed.encode()).await;
  let errored = download_when(&api, failing.name(), id, "errored", ended).await;
  assert_eq!(errored.state, completed(TransferStates::Errored));
  assert_eq!(
    errored.place_in_queue, None,
    "no place once out of the peer's queue"
  );
  assert_eq!((errored.attempts, errored.next_attempt_at), (2, None));
  let query: EventsQuery = serde_json::from_value(json!({ "type": "DownloadFileFailed" })).unwrap();
  let started = Instant::now();
  let failures = loop {
    let failures = api.events(&query).await.unwrap().items;
    if failures.len() >= 2 {
      break failures;
    }
    assert!(started.elapsed() < PATIENCE, "{failures:?}");
    tokio::time::sleep(Duration::from_millis(20)).await;
  };
  let reasons: Vec<serde_json::Value> = failures
    .iter()
    .map(|e| serde_json::to_value(e).unwrap()["data"]["reason"].clone())
    .collect();
  assert!(reasons.contains(&json!("File not shared.")), "{reasons:?}");
  assert!(
    reasons.contains(&json!("Download reported as failed by remote client")),
    "once, after the last try: {reasons:?}"
  );

  // a size the peer gives differently is taken when nothing was written yet
  let sending = Scripted::start(&server).await;
  let grown = "@@share\\b\\grown.flac";
  let id = enqueue(&api, sending.name(), grown, 100).await;
  let mut side = sending.connected().await;
  queued(&mut side, grown).await;
  offer(&mut side, grown, 7, 120).await;
  send_from(listen, sending.name(), 7, 0, 120).await;
  let done = download_when(&api, sending.name(), id, "moved", |t| {
    t.local_path.is_some()
  })
  .await;
  assert_eq!(
    (done.state, done.size),
    (completed(TransferStates::Succeeded), 120)
  );
  let landed = std::fs::read(done.local_path.unwrap()).unwrap();
  assert_eq!(landed, content(120));

  // a partial file is resumed where it ends, and kept when the size differs
  let incomplete = dir
    .path()
    .join("data")
    .join("incomplete")
    .join(sending.name())
    .join("b");
  std::fs::create_dir_all(&incomplete).unwrap();
  std::fs::write(incomplete.join("partial.flac"), &content(100)[..40]).unwrap();
  let partial = "@@share\\b\\partial.flac";
  let id = enqueue(&api, sending.name(), partial, 100).await;
  queued(&mut side, partial).await;
  offer(&mut side, partial, 8, 100).await;
  send_from(listen, sending.name(), 8, 40, 100).await;
  let done = download_when(&api, sending.name(), id, "moved", |t| {
    t.local_path.is_some()
  })
  .await;
  assert_eq!(
    std::fs::read(done.local_path.unwrap()).unwrap(),
    content(100)
  );
  // the move took the emptied directory away
  std::fs::create_dir_all(&incomplete).unwrap();
  std::fs::write(incomplete.join("whole.flac"), content(100)).unwrap();
  let whole = "@@share\\b\\whole.flac";
  let id = enqueue(&api, sending.name(), whole, 100).await;
  queued(&mut side, whole).await;
  offer(&mut side, whole, 9, 100).await;
  send_from(listen, sending.name(), 9, 0, 100).await;
  download_when(&api, sending.name(), id, "moved", |t| {
    t.local_path.is_some()
  })
  .await;
  // the move took the emptied directory away
  std::fs::create_dir_all(&incomplete).unwrap();
  std::fs::write(incomplete.join("shrunk.flac"), &content(100)[..10]).unwrap();
  let shrunk = "@@share\\b\\shrunk.flac";
  let id = enqueue(&api, sending.name(), shrunk, 100).await;
  queued(&mut side, shrunk).await;
  let request = PeerMessage::TransferRequest(TransferRequest {
    direction: Wire::Upload,
    token: 10,
    filename: shrunk.into(),
    file_size: Some(90),
  });
  side.send(&request.encode()).await;
  let aborted = download_when(&api, sending.name(), id, "aborted", ended).await;
  assert_eq!(aborted.state, completed(TransferStates::Aborted));
  assert_eq!(
    aborted.exception.as_deref(),
    Some("the remote size of 90 does not match expected size 100")
  );
  assert_eq!(aborted.attempts, 1, "a size dispute is not tried again");
  assert!(
    incomplete.join("shrunk.flac").exists(),
    "the partial stays for later"
  );

  // the place in queue, asked and said unasked
  let queue = "@@share\\c\\queued.flac";
  let id = enqueue(&api, sending.name(), queue, 100).await;
  queued(&mut side, queue).await;
  download_when(&api, sending.name(), id, "queued", |t| {
    t.state == TransferStates::Queued | TransferStates::Remotely
  })
  .await;
  let peer = sending.name().to_string();
  let api_asks = {
    let api = api.clone();
    tokio::spawn(async move { api.place_in_queue(&peer, id).await })
  };
  assert_eq!(
    side.read_peer().await,
    PeerMessage::PlaceInQueueRequest(PlaceInQueueRequest {
      filename: queue.into()
    })
  );
  let place = |place| {
    PeerMessage::PlaceInQueueResponse(PlaceInQueueResponse {
      filename: queue.into(),
      place,
    })
    .encode()
  };
  side.send(&place(3)).await;
  assert_eq!(api_asks.await.unwrap().unwrap(), 3);
  assert_eq!(
    api.place_in_queue(sending.name(), id).await.unwrap(),
    3,
    "asked again at once, the last answer is given"
  );
  assert!(
    side.quiet_for(Duration::from_millis(200)).await,
    "without asking the peer"
  );
  side.send(&place(2)).await;
  download_when(&api, sending.name(), id, "moved up", |t| {
    t.place_in_queue == Some(2)
  })
  .await;
}

#[tokio::test]
async fn a_peer_that_never_answers_times_out() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let config = "[transfers.download]\nrequest_style = \"transfer_request\"\n\
                [transfers.download.retry]\nattempts = 1\n\
                [soulseek.connection.timeout]\ntransfer_response = \"1s\"\ninactivity = \"2s\"\n";
  let (_daemon, api, _) = started(dir.path(), &server, config).await;

  let silent = Scripted::start(&server).await;
  let file = "@@share\\d\\silence.flac";
  let id = enqueue(&api, silent.name(), file, 10).await;
  let mut side = silent.connected().await;
  let PeerMessage::TransferRequest(asked) = side.read_peer().await else {
    panic!("the daemon asks for the file with a transfer request");
  };
  assert_eq!(asked.direction, Wire::Download);
  let timed_out = download_when(&api, silent.name(), id, "timed out", ended).await;
  assert_eq!(timed_out.state, completed(TransferStates::TimedOut));

  let slow = Scripted::start(&server).await;
  let file = "@@share\\d\\slow.flac";
  let id = enqueue(&api, slow.name(), file, 10).await;
  let mut side = slow.connected().await;
  let PeerMessage::TransferRequest(asked) = side.read_peer().await else {
    panic!("the daemon asks for the file with a transfer request");
  };
  let queued = PeerMessage::TransferResponse(TransferResponse::Denied {
    token: asked.token,
    message: "Queued.".into(),
  });
  side.send(&queued.encode()).await;
  download_when(&api, slow.name(), id, "queued", |t| {
    t.state == TransferStates::Queued | TransferStates::Remotely
  })
  .await;
  // the peer keeps the connection alive past the daemon's wait, and never answers
  let chatting = tokio::spawn(async move {
    let PeerMessage::PlaceInQueueRequest(_) = side.read_peer().await else {
      panic!("the daemon asks the place in queue");
    };
    tokio::time::sleep(Duration::from_secs(1)).await;
    let unrelated = PeerMessage::PlaceInQueueResponse(PlaceInQueueResponse {
      filename: "@@share\\d\\other.flac".into(),
      place: 1,
    });
    side.send(&unrelated.encode()).await;
    side
  });
  let unanswered = api.place_in_queue(slow.name(), id).await.unwrap_err();
  chatting.await.unwrap();
  assert_eq!(
    unanswered.status().map(|s| s.as_u16()),
    Some(504),
    "{unanswered:?}"
  );
}
