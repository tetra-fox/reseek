//! The upload engine against the in-memory daemon, which is not connected: every refusal a
//! peer can be told and what triggers it, a privileged user past the limits, the upload calls
//! of the API, a group we put a user in taking effect at once, and the queued uploads of a user
//! canceled when the user is blocked.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::task::Poll;

use api_types::transfers::{TransferDirection, TransferOutcome, TransferStates, UserResponse};
use reseek_shares::lock;
use serde_json::{Value, json};
use soulseek_client::ClientError;
use soulseek_proto::RemoteFilename;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{InFlight, OVERWHELMED};
use crate::network::resolvers::tests::{edges, exposed, from};
use crate::network::resolvers::{enqueue_download, place_in_queue};
use crate::testing::{TestApp, app};
use crate::transfers::fixtures::{Row, insert};
use crate::transfers::state::State;
use crate::transfers::{Record, store};

const SHARED: &str = "Music\\Album\\01.mp3";

/// A configuration sharing `dir/music` as `Music`, with loopback an administrator, and `extra`.
fn config(dir: &Path, extra: &str) -> String {
  let album = dir.join("music").join("Album");
  std::fs::create_dir_all(&album).unwrap();
  std::fs::write(album.join("01.mp3"), "one").unwrap();
  std::fs::write(album.join("02.mp3"), "two").unwrap();
  format!(
    "[shares]\ndirectories = [\"[Music]{}\"]\n\
     [web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n{extra}",
    dir.join("music").display()
  )
}

async fn shared_app(extra: &str) -> (tempfile::TempDir, TestApp) {
  let files = tempfile::tempdir().unwrap();
  let app = app(&config(files.path(), extra), &[]).await;
  (files, app)
}

/// What `username` asking for `filename` is told: `None` when it is queued.
async fn asked(app: &TestApp, username: &str, filename: &str) -> Option<String> {
  let peer = from("198.51.100.7");
  match enqueue_download(&edges(app), username, peer, &RemoteFilename::from(filename)).await {
    Ok(()) => None,
    Err(ClientError::DownloadEnqueue(reason)) => Some(reason),
    Err(other) => panic!("not a refusal: {other}"),
  }
}

/// The value of the metric `name` in `app`'s exposition.
fn metric(app: &TestApp, name: &str) -> String {
  exposed(app)
    .lines()
    .find_map(|line| line.strip_prefix(&format!("{name} ")).map(str::to_string))
    .unwrap_or_else(|| panic!("no {name}"))
}

#[tokio::test]
async fn every_refusal_a_peer_can_be_told_and_what_triggers_it() {
  let (files, app) =
    shared_app("[transfers.upload.limits]\nqueued = { files = 1 }\ndaily = { failures = 1 }\n")
      .await;
  let not_shared = Some("File not shared.".to_string());
  assert_eq!(
    asked(&app, "alice", "Music\\..\\secret.txt").await,
    not_shared
  );
  assert_eq!(
    asked(&app, "alice", "Music\\Album\\03.mp3").await,
    not_shared
  );
  let block = json!({ "scopes": ["uploads"] });
  let (status, _) = app
    .call("PUT", "/api/v0/users/mallory/block", Some(block))
    .await;
  assert_eq!(status, 200);
  assert_eq!(asked(&app, "mallory", SHARED).await, not_shared);

  // a shared file gone from disk is not shared, and the shares are told to scan
  std::fs::remove_file(files.path().join("music/Album/02.mp3")).unwrap();
  assert_eq!(
    asked(&app, "alice", "Music\\Album\\02.mp3").await,
    not_shared
  );
  assert!(app.state.state.current().shares.scan_pending);

  let mut queued = Row::new(
    "carol",
    TransferDirection::Upload,
    TransferStates::QueuedLocally,
  );
  queued.filename = "Music\\Album\\02.mp3".to_string();
  let mut failed = Row::new(
    "dave",
    TransferDirection::Upload,
    TransferStates::Completed(TransferOutcome::Cancelled),
  );
  failed.started_at = Some(chrono::Utc::now());
  failed.ended_at = failed.started_at;
  insert(&app.state.transfers, vec![queued, failed]).await;
  assert_eq!(
    asked(&app, "carol", SHARED).await.as_deref(),
    Some("Too many files")
  );
  assert_eq!(
    asked(&app, "dave", SHARED).await.as_deref(),
    Some("Too many failed transfers today")
  );

  // too many waiting, at the pace of the last ones
  let intake = &app.state.uploads.intake;
  intake.waiting.store(2, Ordering::Release);
  intake.latency.store(90_001f64.to_bits(), Ordering::Release);
  assert_eq!(
    asked(&app, "erin", SHARED).await.as_deref(),
    Some(OVERWHELMED)
  );
  intake.waiting.store(0, Ordering::Release);

  assert_eq!(metric(&app, "reseek_enqueue_requests_received"), "7");
  assert_eq!(metric(&app, "reseek_enqueue_requests_rejected"), "7");
  assert_eq!(metric(&app, "reseek_enqueue_requests_dropped"), "1");
}

#[tokio::test]
async fn a_privileged_user_is_queued_past_the_limits() {
  let (_files, app) = shared_app("[transfers.upload.limits]\nqueued = { files = 1 }\n").await;
  let mut queued = Row::new(
    "vip",
    TransferDirection::Upload,
    TransferStates::QueuedLocally,
  );
  queued.filename = "Music\\Album\\02.mp3".to_string();
  insert(&app.state.transfers, vec![queued]).await;
  assert_eq!(
    asked(&app, "vip", SHARED).await.as_deref(),
    Some("Too many files")
  );
  app
    .state
    .users
    .privileged_listed(&["vip".to_string()])
    .await;
  assert_eq!(asked(&app, "vip", SHARED).await, None);
  let (status, body) = app.call("GET", "/api/v0/transfers/uploads/vip", None).await;
  assert_eq!(status, 200);
  let user: UserResponse = serde_json::from_value(body).unwrap();
  let files: Vec<&str> = user.directories[0]
    .files
    .iter()
    .map(|f| f.filename.as_str())
    .collect();
  assert!(files.contains(&SHARED), "{files:?}");
  assert_eq!(metric(&app, "reseek_enqueue_requests_accepted"), "1");
}

#[tokio::test]
async fn a_running_upload_stands_for_a_new_request_and_an_ended_one_is_superseded() {
  let (_files, app) = shared_app("").await;
  let mut queued = Row::new(
    "alice",
    TransferDirection::Upload,
    TransferStates::QueuedLocally,
  );
  queued.filename = SHARED.to_string();
  let old = queued.id;
  insert(&app.state.transfers, vec![queued]).await;
  let uploads = &app.state.uploads;
  lock(&uploads.in_flight).insert(
    old,
    InFlight {
      cancel: CancellationToken::new(),
      remove: false,
      reason: None,
      ended: false,
    },
  );
  let listed = async || -> Vec<Uuid> {
    let (_, body) = app
      .call("GET", "/api/v0/transfers/uploads/alice", None)
      .await;
    let user: UserResponse = serde_json::from_value(body).unwrap();
    user
      .directories
      .iter()
      .flat_map(|d| d.files.iter().map(|f| f.id))
      .collect()
  };
  assert_eq!(asked(&app, "alice", SHARED).await, None);
  assert_eq!(listed().await, [old], "the running upload stands for it");

  // the client ended the upload and told the peer, which asks again before the task wrote it
  lock(&uploads.in_flight).get_mut(&old).unwrap().ended = true;
  assert_eq!(asked(&app, "alice", SHARED).await, None);
  let now = listed().await;
  assert_eq!(now.len(), 1);
  assert_ne!(now[0], old, "a new upload");
  let superseded = app
    .state
    .transfers
    .read(move |conn| store::get(conn, old))
    .await
    .unwrap()
    .unwrap();
  assert!(superseded.removed);
}

#[tokio::test]
async fn a_peer_first_in_line_is_told_place_one() {
  let (_files, app) = shared_app("").await;
  app
    .state
    .uploads
    .queue
    .enqueue(Uuid::new_v4(), "alice", SHARED);
  let peer = from("198.51.100.7");
  let place =
    |filename: &str| place_in_queue(&edges(&app), "alice", peer, &RemoteFilename::from(filename));
  assert_eq!(place(SHARED), Some(1));
  assert_eq!(place("Music\\Album\\02.mp3"), None);
}

#[tokio::test]
async fn the_upload_calls_find_an_upload_by_its_user_and_id() {
  let (_files, app) = shared_app("").await;
  let (status, body) = app
    .call("GET", "/api/v0/transfers/uploads/nobody", None)
    .await;
  assert_eq!(status, 200, "a user without uploads has none");
  assert_eq!(body, json!({ "username": "nobody", "directories": [] }));

  let mut done = Record::upload("alice", SHARED, 3);
  done.state = State::Completed(TransferOutcome::Succeeded);
  done.ended_at = Some(chrono::Utc::now());
  let written = done.clone();
  app
    .state
    .transfers
    .write(move |conn| store::insert(conn, &written))
    .await
    .unwrap();
  let id = done.id;
  let (status, _) = app
    .call("GET", &format!("/api/v0/transfers/uploads/bob/{id}"), None)
    .await;
  assert_eq!(status, 404, "another user's id");
  let (status, why) = app
    .call(
      "GET",
      &format!("/api/v0/transfers/uploads/alice/{id}/why"),
      None,
    )
    .await;
  assert_eq!(status, 200);
  assert_eq!(why["position"], Value::Null, "not in the queue");
  assert_eq!(why["group"], "default");
  assert_eq!(why["limits"]["queued"]["failures"], Value::Null);
  assert_eq!(
    why["limits"]["daily"]["files"],
    json!({ "used": 0, "limit": null })
  );
  let (status, _) = app
    .call(
      "POST",
      &format!("/api/v0/transfers/uploads/alice/{id}/bump"),
      None,
    )
    .await;
  assert_eq!(status, 409, "a finished upload waits for nothing");

  let (status, _) = app
    .call(
      "DELETE",
      &format!("/api/v0/transfers/uploads/alice/{id}?remove=true"),
      None,
    )
    .await;
  assert_eq!(status, 204);
  let removed = app
    .state
    .transfers
    .read(move |conn| store::get(conn, id))
    .await
    .unwrap()
    .unwrap();
  assert!(removed.removed);
}

#[tokio::test]
async fn a_group_we_put_a_user_in_takes_effect_at_once() {
  let (_files, app) = shared_app("[transfers.groups.user_defined.vip.upload]\nslots = 5\n").await;
  assert_eq!(app.state.users.queue_group("zoe"), "default");
  let (status, _) = app
    .call(
      "PUT",
      "/api/v0/users/zoe/groups",
      Some(json!({ "groups": ["vip"] })),
    )
    .await;
  assert_eq!(status, 204);
  assert_eq!(app.state.users.queue_group("zoe"), "vip");
  assert_eq!(app.state.uploads.forecast("zoe").group, "vip");
  let (status, _) = app
    .call(
      "PUT",
      "/api/v0/users/zoe/groups",
      Some(json!({ "groups": [] })),
    )
    .await;
  assert_eq!(status, 204);
  assert_eq!(app.state.users.queue_group("zoe"), "default");
}

#[tokio::test]
async fn blocking_a_user_cancels_their_queued_uploads() {
  let (_files, app) = shared_app("").await;
  let uploads = &app.state.uploads;
  let (queued, other) = (Uuid::new_v4(), Uuid::new_v4());
  let (cancel, kept) = (CancellationToken::new(), CancellationToken::new());
  for (id, username, cancel) in [(queued, "mallory", &cancel), (other, "alice", &kept)] {
    uploads.queue.enqueue(id, username, SHARED);
    lock(&uploads.in_flight).insert(
      id,
      InFlight {
        cancel: cancel.clone(),
        remove: false,
        reason: None,
        ended: false,
      },
    );
  }
  let block = json!({ "scopes": ["uploads"] });
  let (status, _) = app
    .call("PUT", "/api/v0/users/mallory/block", Some(block))
    .await;
  assert_eq!(status, 200);
  assert!(cancel.is_cancelled());
  assert!(!kept.is_cancelled());
  assert_eq!(
    lock(&uploads.in_flight).get(&queued).unwrap().reason,
    Some(super::BLOCKED)
  );
}

#[tokio::test]
async fn a_file_sent_to_a_user_unasked_is_refused_unless_it_is_shared() {
  let (_files, app) = shared_app("").await;
  let (status, answer) = app
    .call(
      "POST",
      "/api/v0/transfers/uploads/alice",
      Some(json!({ "files": [SHARED, "@@nope\\a.flac"] })),
    )
    .await;
  assert_eq!(status, 200, "{answer}");
  assert_eq!(answer["queued"], json!([SHARED]));
  assert_eq!(
    answer["failed"],
    json!([{ "filename": "@@nope\\a.flac", "message": "File not shared." }])
  );
}

/// The uploads of `SHARED` to alice, removed ones included, oldest first.
async fn uploads_of_shared(app: &TestApp) -> Vec<Record> {
  app
    .state
    .transfers
    .read(|conn| {
      let mut ids = conn.prepare(
        "SELECT Id FROM Transfers WHERE Username = 'alice' AND Filename = ?1 ORDER BY rowid",
      )?;
      let ids = ids
        .query_map([SHARED], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
      ids
        .into_iter()
        .map(|id| store::get(conn, id.parse().unwrap()).map(Option::unwrap))
        .collect()
    })
    .await
    .unwrap()
}

/// A send dropped while the transfers database is held, before its record can be written,
/// still queues its file: the record is written and a task runs the upload to its end.
#[tokio::test]
async fn a_send_dropped_before_its_record_is_written_still_queues_the_file() {
  let (_files, app) = shared_app("").await;
  let uploads = &app.state.uploads;
  // in-memory databases send reads through the writer too, so while it is held the send can
  // neither read the user's limits nor write the record
  let (release, released) = std::sync::mpsc::channel::<()>();
  let (holding, held) = tokio::sync::oneshot::channel();
  let transfers = app.state.transfers.clone();
  let hold = tokio::spawn(async move {
    transfers
      .write(move |_| {
        holding.send(()).unwrap();
        released.recv().unwrap();
        Ok(())
      })
      .await
  });
  held.await.unwrap();
  let mut sending = Box::pin(uploads.send("alice", vec![SHARED.to_string()]));
  for _ in 0..5 {
    tokio::task::yield_now().await;
    let polled = std::future::poll_fn(|cx| Poll::Ready(sending.as_mut().poll(cx))).await;
    assert!(polled.is_pending());
  }
  drop(sending);
  release.send(()).unwrap();
  hold.await.unwrap().unwrap();
  let settled = tokio::time::timeout(std::time::Duration::from_secs(5), async {
    loop {
      let written = uploads_of_shared(&app).await;
      if written.iter().any(Record::is_completed) && lock(&uploads.in_flight).is_empty() {
        return written;
      }
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await;
  let Ok(written) = settled else {
    panic!(
      "the dropped send left {:?} with {} in flight",
      uploads_of_shared(&app)
        .await
        .iter()
        .map(|r| r.state.kind())
        .collect::<Vec<_>>(),
      lock(&uploads.in_flight).len()
    );
  };
  assert_eq!(written.len(), 1);
  let id = written[0].id;

  let uri = format!("/api/v0/transfers/uploads/alice/{id}");
  let (status, _) = app.call("DELETE", &uri, None).await;
  assert_eq!(status, 204);
  let sent = uploads.send("alice", vec![SHARED.to_string()]).await;
  assert_eq!(sent.queued, [SHARED]);
  let now = uploads_of_shared(&app).await;
  assert_eq!(
    now.len(),
    2,
    "a new upload, not the dropped one standing for it"
  );
}

#[tokio::test]
async fn canceling_an_upload_no_task_runs_ends_it() {
  let (_files, app) = shared_app("").await;
  let waiting = Row::new(
    "alice",
    TransferDirection::Upload,
    TransferStates::QueuedLocally,
  );
  let id = waiting.id;
  insert(&app.state.transfers, vec![waiting]).await;
  let uri = format!("/api/v0/transfers/uploads/alice/{id}");
  let (status, _) = app.call("DELETE", &uri, None).await;
  assert_eq!(status, 204);
  let ended = app
    .state
    .transfers
    .read(move |conn| store::get(conn, id))
    .await
    .unwrap()
    .unwrap();
  assert_eq!(
    ended.state.kind(),
    TransferStates::Completed(TransferOutcome::Cancelled)
  );
  assert!(ended.ended_at.is_some());
}
