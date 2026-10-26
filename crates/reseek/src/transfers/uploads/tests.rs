//! The upload engine against the in-memory daemon, which is not connected: every refusal a
//! peer can be told and what triggers it, a privileged user past the limits, the upload calls
//! of the API, a group we put a user in taking effect at once, and the queued uploads of a user
//! cancelled when the user is blocked.

use std::path::Path;
use std::sync::atomic::Ordering;

use api_types::transfers::{TransferDirection, TransferStates, UserResponse};
use serde_json::{Value, json};
use soulseek_client::ClientError;
use soulseek_proto::RemoteFilename;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{InFlight, OVERWHELMED, lock};
use crate::network::resolvers::tests::{edges, exposed, from};
use crate::network::resolvers::{enqueue_download, place_in_queue};
use crate::testing::{TestApp, app, request};
use crate::transfers::fixtures::{Row, insert};
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

async fn send(app: &TestApp, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
  let answer = app.send(request(method, uri, body)).await;
  let json = if answer.body.is_empty() {
    Value::Null
  } else {
    serde_json::from_slice(&answer.body).unwrap_or(Value::Null)
  };
  (answer.status.as_u16(), json)
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
  let (status, _) = send(&app, "PUT", "/api/v0/users/mallory/block", Some(block)).await;
  assert_eq!(status, 200);
  assert_eq!(asked(&app, "mallory", SHARED).await, not_shared);

  // a shared file gone from disk is not shared, and the shares are told to scan
  std::fs::remove_file(files.path().join("music/Album/02.mp3")).unwrap();
  assert_eq!(
    asked(&app, "alice", "Music\\Album\\02.mp3").await,
    not_shared
  );
  assert!(app.state.state.current().shares.scan_pending);

  let mut queued = Row::new("carol", TransferDirection::Upload, 2 | 2048);
  queued.filename = "Music\\Album\\02.mp3".to_string();
  let mut failed = Row::new("dave", TransferDirection::Upload, 16 | 64);
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
async fn each_limit_tells_the_peer_its_own_reason() {
  let now = chrono::Utc::now();
  // a row of alice's: its state bits, when it started, and its size
  let row = |state: u32, started: bool, size: i64| {
    let mut row = Row::new("alice", TransferDirection::Upload, state);
    row.filename = "Music\\Album\\02.mp3".to_string();
    row.size = size;
    row.started_at = started.then_some(now);
    row.ended_at = (state & 16 != 0).then_some(now);
    row
  };
  let cases = [
    (
      "queued = { files = 1 }",
      row(2 | 2048, false, 1),
      "Too many files",
    ),
    (
      "queued = { megabytes = 1 }",
      row(2 | 2048, false, 999_998),
      "Too many megabytes",
    ),
    (
      "weekly = { failures = 1 }",
      row(16 | 256, true, 1),
      "Too many failed transfers this week",
    ),
    (
      "weekly = { files = 1 }",
      row(16 | 32, true, 1),
      "Too many files this week",
    ),
    (
      "weekly = { megabytes = 1 }",
      row(16 | 32, true, 999_998),
      "Too many megabytes this week",
    ),
    (
      "daily = { failures = 1 }",
      row(16 | 512, true, 1),
      "Too many failed transfers today",
    ),
    (
      "daily = { files = 1 }",
      row(16 | 32, true, 1),
      "Too many files today",
    ),
    (
      "daily = { megabytes = 1 }",
      row(16 | 32, true, 999_998),
      "Too many megabytes today",
    ),
  ];
  for (limit, row, reason) in cases {
    let (_files, app) = shared_app(&format!("[transfers.upload.limits]\n{limit}\n")).await;
    insert(&app.state.transfers, vec![row]).await;
    assert_eq!(
      asked(&app, "alice", SHARED).await.as_deref(),
      Some(reason),
      "{limit}"
    );
    assert_eq!(
      asked(&app, "bob", SHARED).await,
      None,
      "{limit}: bob has nothing yet"
    );
  }
}

#[tokio::test]
async fn a_privileged_user_is_queued_past_the_limits() {
  let (_files, app) = shared_app("[transfers.upload.limits]\nqueued = { files = 1 }\n").await;
  let mut queued = Row::new("vip", TransferDirection::Upload, 2 | 2048);
  queued.filename = "Music\\Album\\02.mp3".to_string();
  insert(&app.state.transfers, vec![queued]).await;
  assert_eq!(
    asked(&app, "vip", SHARED).await.as_deref(),
    Some("Too many files")
  );
  app.state.users.privileged_listed(&["vip".to_string()]);
  assert_eq!(asked(&app, "vip", SHARED).await, None);
  let (status, body) = send(&app, "GET", "/api/v0/transfers/uploads/vip", None).await;
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
  let mut queued = Row::new("alice", TransferDirection::Upload, 2 | 2048);
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
    let (_, body) = send(&app, "GET", "/api/v0/transfers/uploads/alice", None).await;
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
  let (status, body) = send(&app, "GET", "/api/v0/transfers/uploads/nobody", None).await;
  assert_eq!(status, 200, "a user without uploads has none");
  assert_eq!(body, json!({ "username": "nobody", "directories": [] }));

  let mut done = Record::upload("alice", SHARED, 3);
  done.state = TransferStates::Completed | TransferStates::Succeeded;
  done.ended_at = Some(chrono::Utc::now());
  let written = done.clone();
  app
    .state
    .transfers
    .write(move |conn| store::insert(conn, &written))
    .await
    .unwrap();
  let id = done.id;
  let unknown = Uuid::new_v4();
  for (method, uri) in [
    ("GET", format!("/api/v0/transfers/uploads/alice/{unknown}")),
    ("GET", format!("/api/v0/transfers/uploads/bob/{id}")),
    (
      "DELETE",
      format!("/api/v0/transfers/uploads/alice/{unknown}"),
    ),
    (
      "GET",
      format!("/api/v0/transfers/uploads/alice/{unknown}/why"),
    ),
    (
      "POST",
      format!("/api/v0/transfers/uploads/alice/{unknown}/bump"),
    ),
  ] {
    let (status, _) = send(&app, method, &uri, None).await;
    assert_eq!(status, 404, "{method} {uri}");
  }
  let (status, why) = send(
    &app,
    "GET",
    &format!("/api/v0/transfers/uploads/alice/{id}/why"),
    None,
  )
  .await;
  assert_eq!(status, 200);
  assert_eq!(why["position"], Value::Null, "not in the queue");
  assert_eq!(why["group"], "default");
  assert_eq!(why["globalSlots"], 10);
  assert_eq!(why["limits"]["queued"]["failures"], Value::Null);
  assert_eq!(
    why["limits"]["daily"]["files"],
    json!({ "used": 0, "limit": null })
  );
  let (status, _) = send(
    &app,
    "POST",
    &format!("/api/v0/transfers/uploads/alice/{id}/bump"),
    None,
  )
  .await;
  assert_eq!(status, 409, "a finished upload waits for nothing");

  let (status, _) = send(
    &app,
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
async fn the_grouped_uploads_take_a_range_of_request_times_and_a_limit() {
  let (_files, app) = shared_app("").await;
  let rows = (1..=3)
    .map(|minute| {
      let mut row = Row::new("alice", TransferDirection::Upload, 48);
      row.filename = format!("Music\\{minute}.mp3");
      row.requested_at = format!("2026-01-01T00:0{minute}:00Z").parse().unwrap();
      row
    })
    .collect();
  insert(&app.state.transfers, rows).await;
  let uri = "/api/v0/transfers/uploads?since=2026-01-01T00:02:00Z&limit=1";
  let (status, body) = send(&app, "GET", uri, None).await;
  assert_eq!(status, 200);
  assert_eq!(
    body[0]["directories"][0]["files"][0]["filename"],
    "Music\\2.mp3"
  );
  assert_eq!(body[0]["directories"][0]["fileCount"], 1);
}

#[tokio::test]
async fn a_group_we_put_a_user_in_takes_effect_at_once() {
  let (_files, app) =
    shared_app("[transfers.groups.user_defined.vip.upload]\npriority = 5\n").await;
  assert_eq!(app.state.users.group("zoe"), "default");
  let (status, _) = send(
    &app,
    "PUT",
    "/api/v0/users/zoe/group",
    Some(json!({ "group": "vip" })),
  )
  .await;
  assert_eq!(status, 204);
  assert_eq!(app.state.users.group("zoe"), "vip");
  assert_eq!(app.state.uploads.forecast("zoe").group, "vip");
  let (status, _) = send(
    &app,
    "PUT",
    "/api/v0/users/zoe/group",
    Some(json!({ "group": null })),
  )
  .await;
  assert_eq!(status, 204);
  assert_eq!(app.state.users.group("zoe"), "default");
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
  let (status, _) = send(&app, "PUT", "/api/v0/users/mallory/block", Some(block)).await;
  assert_eq!(status, 200);
  assert!(cancel.is_cancelled());
  assert!(!kept.is_cancelled());
  assert_eq!(
    lock(&uploads.in_flight).get(&queued).unwrap().reason,
    Some(super::BLOCKED)
  );
}
