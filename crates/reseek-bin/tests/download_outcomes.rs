//! Downloads by the daemon from scripted peers logged in to soulfind, for what a real client
//! does not do on cue: a refusal, a failure tried again after its wait, a resumed partial, a
//! partial kept when the peer names another size, a peer that never answers the request or the
//! question of its queue, the place in its queue a peer gives when asked or on its own, and a
//! script told how each download ended.

mod support;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use api_client::Client;
use api_types::transfers::{Transfer, TransferOutcome, TransferStates};
use reseek::safety::Os;
use reseek::transfers::downloads::incomplete_path;
use serde_json::json;
use soulseek_proto::peer::{
  PlaceInQueueRequest, PlaceInQueueResponse, QueueDownload, TransferRequest, TransferResponse,
  UploadDenied, UploadFailed,
};
use soulseek_proto::{PeerMessage, TransferDirection as Wire};
use support::peer::PeerSide;
use support::scripted::{Scripted, send_from};
use support::soulfind::{self, Soulfind};
use support::transfers::content;
use support::{
  daemon_listening, download_when, enqueue, events, free_port, logged_in, signed_in, until,
};

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

fn ended(transfer: &Transfer) -> bool {
  transfer.ended_at.is_some()
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
  assert_eq!(
    rejected.state,
    TransferStates::Completed(TransferOutcome::Rejected)
  );
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
    t.state == TransferStates::QueuedRemotely
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
  assert_eq!(waiting.state, TransferStates::QueuedLocally);
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
    t.state == TransferStates::QueuedRemotely
  })
  .await;
  side.send(&place(4)).await;
  download_when(&api, failing.name(), id, "placed", |t| {
    t.place_in_queue == Some(4)
  })
  .await;
  side.send(&failed.encode()).await;
  let errored = download_when(&api, failing.name(), id, "errored", ended).await;
  assert_eq!(
    errored.state,
    TransferStates::Completed(TransferOutcome::Errored)
  );
  assert_eq!(
    errored.place_in_queue, None,
    "no place once out of the peer's queue"
  );
  assert_eq!((errored.attempts, errored.next_attempt_at), (2, None));
  let reasons: Vec<serde_json::Value> = events(&api, "DownloadFileFailed", 2)
    .await
    .iter()
    .map(|data| data["reason"].clone())
    .collect();
  assert!(reasons.contains(&json!("File not shared.")), "{reasons:?}");
  assert!(
    reasons.contains(&json!("Download reported as failed by remote client")),
    "{reasons:?}"
  );
  assert_eq!(reasons.len(), 2, "once, after the last try: {reasons:?}");

  // a partial file is resumed where it ends, and kept when the size differs
  let sending = Scripted::start(&server).await;
  let partial_at = |filename: &str| {
    let root = dir.path().join("data").join("incomplete");
    let (path, _) = incomplete_path(&root, sending.name(), filename, Os::CURRENT).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    path
  };
  let partial = "@@share\\b\\partial.flac";
  std::fs::write(partial_at(partial), &content(100)[..40]).unwrap();
  let id = enqueue(&api, sending.name(), partial, 100).await;
  let mut side = sending.connected().await;
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
  let shrunk = "@@share\\b\\shrunk.flac";
  let kept_at = partial_at(shrunk);
  std::fs::write(&kept_at, &content(100)[..10]).unwrap();
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
  assert_eq!(
    aborted.state,
    TransferStates::Completed(TransferOutcome::Aborted)
  );
  assert_eq!(
    aborted.exception.as_deref(),
    Some("the remote size of 90 does not match expected size 100")
  );
  assert_eq!(aborted.attempts, 1, "a size dispute is not tried again");
  assert!(kept_at.exists(), "the partial stays for later");

  // the place in queue, asked and said unasked
  let queue = "@@share\\c\\queued.flac";
  let id = enqueue(&api, sending.name(), queue, 100).await;
  queued(&mut side, queue).await;
  download_when(&api, sending.name(), id, "queued", |t| {
    t.state == TransferStates::QueuedRemotely
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
  assert_eq!(
    timed_out.state,
    TransferStates::Completed(TransferOutcome::TimedOut)
  );

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
    t.state == TransferStates::QueuedRemotely
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

#[tokio::test]
async fn a_script_learns_how_each_download_ended_and_its_runs_are_recorded() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let config = r#"
[integrations.scripts.outcome]
on = ["DownloadFileComplete", "DownloadFileFailed"]
[integrations.scripts.outcome.run]
executable = "/bin/sh"
arglist = ["-c", 'echo "$RESEEK_SCRIPT_RESULT: $RESEEK_SCRIPT_REASON"; test "$RESEEK_SCRIPT_RESULT" = succeeded']
"#;
  let (_daemon, api, listen) = started(dir.path(), &server, config).await;

  let refusing = Scripted::start(&server).await;
  let refused = "@@share\\e\\no.flac";
  let id = enqueue(&api, refusing.name(), refused, 10).await;
  let mut side = refusing.connected().await;
  queued(&mut side, refused).await;
  let denied = PeerMessage::UploadDenied(UploadDenied {
    filename: refused.into(),
    reason: "File not shared.".into(),
  });
  side.send(&denied.encode()).await;
  download_when(&api, refusing.name(), id, "rejected", ended).await;

  let sending = Scripted::start(&server).await;
  let sent = "@@share\\e\\yes.flac";
  let id = enqueue(&api, sending.name(), sent, 20).await;
  let mut side = sending.connected().await;
  queued(&mut side, sent).await;
  offer(&mut side, sent, 9, 20).await;
  send_from(listen, sending.name(), 9, 0, 20).await;
  download_when(&api, sending.name(), id, "moved", |t| {
    t.local_path.is_some()
  })
  .await;

  let mut runs = events(&api, "ScriptRunCompleted", 2).await;
  runs.sort_by_key(|run| run["eventType"].as_str().unwrap().to_string());
  let outcome = |run: &serde_json::Value| {
    (
      run["eventType"].clone(),
      run["succeeded"].clone(),
      run["exitCode"].clone(),
      run["output"].clone(),
    )
  };
  assert_eq!(
    outcome(&runs[0]),
    (
      json!("DownloadFileComplete"),
      json!(true),
      json!(0),
      json!("succeeded: ")
    )
  );
  assert_eq!(
    outcome(&runs[1]),
    (
      json!("DownloadFileFailed"),
      json!(false),
      json!(1),
      json!("failed: File not shared.")
    )
  );
  for kind in [
    "DownloadFileComplete",
    "DownloadDirectoryComplete",
    "DownloadFileFailed",
  ] {
    assert_eq!(events(&api, kind, 1).await.len(), 1, "{kind} once");
  }
}
