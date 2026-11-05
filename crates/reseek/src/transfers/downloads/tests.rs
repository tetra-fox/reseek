//! The download calls of the API against the in-memory daemon, which is not connected: finding
//! a download by its peer and id, canceling and removing one no task runs, removing completed
//! ones, a finished download's file served from where it was moved, the removals and updates
//! pushed on the `transfers` topic, and a job told of once its last download ended.

use api_types::transfers::{TransferStates, UserResponse};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::testing::{TRUSTED, TestApp, app, request};
use crate::transfers::{Record, store};

/// Writes `record` into the daemon's `transfers.db`.
async fn insert(app: &TestApp, record: &Record) {
  let record = record.clone();
  app
    .state
    .transfers
    .write(move |conn| store::insert(conn, &record))
    .await
    .unwrap();
}

async fn read(app: &TestApp, id: Uuid) -> Record {
  app
    .state
    .transfers
    .read(move |conn| store::get(conn, id))
    .await
    .unwrap()
    .unwrap()
}

fn done(username: &str, filename: &str) -> Record {
  let mut record = Record::download(username, filename, 3, None);
  record.state = TransferStates::Completed | TransferStates::Succeeded;
  record.ended_at = Some(chrono::Utc::now());
  record
}

#[tokio::test]
async fn a_download_is_found_by_its_peer_and_id_with_its_history() {
  let app = app(TRUSTED, &[]).await;
  let record = Record::download("alice", "@@abcde\\Music\\01.flac", 10, None);
  insert(&app, &record).await;
  let id = record.id;
  let (status, _) = app
    .call(
      "GET",
      &format!("/api/v0/transfers/downloads/bob/{id}"),
      None,
    )
    .await;
  assert_eq!(status, 404, "the peer is part of the path");
  let uri = format!("/api/v0/transfers/downloads/alice/{id}?includeHistory=true");
  let (status, body) = app.call("GET", &uri, None).await;
  assert_eq!(status, 200);
  assert_eq!(body["stateHistory"][0]["state"], "Queued, Locally");
  assert_eq!(body["localPath"], Value::Null);
  let (status, body) = app
    .call("GET", "/api/v0/transfers/downloads/alice", None)
    .await;
  assert_eq!(status, 200);
  let user: UserResponse = serde_json::from_value(body).unwrap();
  assert_eq!(user.directories[0].directory, "@@abcde\\Music");
  let (status, _) = app
    .call("GET", "/api/v0/transfers/downloads/carol", None)
    .await;
  assert_eq!(status, 404, "a user without downloads");
}

#[tokio::test]
async fn canceling_a_download_no_task_runs_ends_it_and_can_remove_it() {
  let app = app(TRUSTED, &[]).await;
  let stuck = Record::download("alice", "a.flac", 10, None);
  insert(&app, &stuck).await;
  let id = stuck.id;
  let uri = format!("/api/v0/transfers/downloads/alice/{id}?remove=true");
  let (status, _) = app.call("DELETE", &uri, None).await;
  assert_eq!(status, 204);
  let ended = read(&app, id).await;
  assert_eq!(
    ended.state,
    TransferStates::Completed | TransferStates::Cancelled
  );
  assert_eq!(
    ended.exception.as_deref(),
    Some("The operation was canceled.")
  );
  assert!(ended.ended_at.is_some() && ended.removed);
  let (_, listed) = app.call("GET", "/api/v0/transfers/downloads", None).await;
  assert_eq!(listed, json!([]), "removed downloads are not listed");
  let (_, listed) = app
    .call(
      "GET",
      "/api/v0/transfers/downloads?includeRemoved=true",
      None,
    )
    .await;
  assert_eq!(listed[0]["username"], "alice");
}

#[tokio::test]
async fn canceling_a_download_no_task_runs_ends_its_wait_and_spares_one_that_ended() {
  let app = app(TRUSTED, &[]).await;
  let mut waiting = Record::download("alice", "a.flac", 10, None);
  waiting.next_attempt_at = Some(chrono::Utc::now());
  waiting.place_in_queue = Some(4);
  insert(&app, &waiting).await;
  let uri = format!("/api/v0/transfers/downloads/alice/{}", waiting.id);
  let (status, _) = app.call("DELETE", &uri, None).await;
  assert_eq!(status, 204);
  let ended = read(&app, waiting.id).await;
  assert_eq!(
    ended.state,
    TransferStates::Completed | TransferStates::Cancelled
  );
  assert_eq!((ended.next_attempt_at, ended.place_in_queue), (None, None));

  // a task that ended the download after the cancel read it keeps how it ended
  let mut finished = done("alice", "b.flac");
  finished.local_path = Some("/music/b.flac".into());
  insert(&app, &finished).await;
  let id = finished.id;
  let canceled = TransferStates::Completed | TransferStates::Cancelled;
  let at = chrono::Utc::now();
  let ended = app
    .state
    .transfers
    .write(move |conn| store::end_unless_ended(conn, id, canceled, at, "canceled"))
    .await
    .unwrap();
  assert!(!ended);
  let kept = read(&app, id).await;
  assert_eq!(
    kept.state,
    TransferStates::Completed | TransferStates::Succeeded
  );
  assert_eq!(kept.local_path.as_deref(), Some("/music/b.flac"));
}

#[tokio::test]
async fn removing_completed_downloads_leaves_running_ones() {
  let app = app(TRUSTED, &[]).await;
  let finished = done("alice", "a.flac");
  let waiting = Record::download("alice", "b.flac", 10, None);
  insert(&app, &finished).await;
  insert(&app, &waiting).await;
  let (status, _) = app
    .call("DELETE", "/api/v0/transfers/downloads/all/completed", None)
    .await;
  assert_eq!(status, 204);
  assert!(read(&app, finished.id).await.removed);
  assert!(!read(&app, waiting.id).await.removed);
  let request = json!({ "ids": [waiting.id, finished.id, Uuid::new_v4()], "action": "cancel" });
  let (status, result) = app
    .call("POST", "/api/v0/transfers/downloads/bulk", Some(request))
    .await;
  assert_eq!(status, 200);
  assert_eq!(result["ok"], json!([waiting.id]));
  assert_eq!(result["failed"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_finished_downloads_file_is_served_from_where_it_was_moved() {
  let app = app(TRUSTED, &[]).await;
  let downloads = app.state.paths.downloads.clone();
  std::fs::create_dir_all(downloads.join("album")).unwrap();
  let file = downloads.join("album").join("01.flac");
  std::fs::write(&file, "abc").unwrap();
  let mut moved = done("alice", "@@abcde\\album\\01.flac");
  moved.local_path = Some(file.display().to_string());
  let unmoved = done("alice", "@@abcde\\album\\02.flac");
  insert(&app, &moved).await;
  insert(&app, &unmoved).await;
  let content = |user: &str, id: Uuid| format!("/api/v0/transfers/downloads/{user}/{id}/content");
  let answer = app
    .send(request("GET", &content("alice", moved.id), None))
    .await;
  assert_eq!(answer.status, 200);
  assert_eq!(&answer.body[..], b"abc");
  let (status, _) = app.call("GET", &content("bob", moved.id), None).await;
  assert_eq!(status, 404, "the peer is part of the path");
  let (status, _) = app.call("GET", &content("alice", unmoved.id), None).await;
  assert_eq!(status, 404, "a download without a file");
  let token = json!({ "transferId": moved.id });
  let (status, body) = app
    .call("POST", "/api/v0/session/file-token", Some(token))
    .await;
  assert_eq!(status, 200, "{body}");
  let token = body["token"].as_str().unwrap();
  let linked = format!("{}?t={token}", content("alice", moved.id));
  let answer = app
    .send_from("192.0.2.1:40000", request("GET", &linked, None))
    .await;
  assert_eq!(answer.status, 200, "the link's token alone lets it in");
  assert_eq!(&answer.body[..], b"abc");
  std::fs::remove_file(&file).unwrap();
  let (status, _) = app.call("GET", &content("alice", moved.id), None).await;
  assert_eq!(status, 404, "a file that is gone");
}

#[tokio::test]
async fn removals_and_updates_are_pushed_on_the_transfers_topic() {
  let app = app(TRUSTED, &[]).await;
  let finished = done("alice", "a.flac");
  let stuck = Record::download("alice", "b.flac", 10, None);
  insert(&app, &finished).await;
  insert(&app, &stuck).await;
  let mut stream = app
    .stream_from(
      "127.0.0.1:1",
      request("GET", "/api/v0/live?topics=transfers", None),
    )
    .await;
  assert_eq!(stream.next().await.unwrap().event, "hello");
  let (status, _) = app
    .call("DELETE", "/api/v0/transfers/downloads/all/completed", None)
    .await;
  assert_eq!(status, 204);
  let removed = stream.next().await.unwrap();
  assert_eq!(removed.event, "transfers.removed");
  assert_eq!(
    removed.data,
    json!({ "id": finished.id, "username": "alice", "direction": "Download" })
  );
  let uri = format!("/api/v0/transfers/downloads/alice/{}", stuck.id);
  app.call("DELETE", &uri, None).await;
  let updated = stream.next().await.unwrap();
  assert_eq!(updated.event, "transfers.updated");
  assert_eq!(updated.data["state"], "Completed, Cancelled");
}

#[tokio::test]
async fn a_job_is_told_once_by_the_cancel_that_ends_its_last_download() {
  let app = app(TRUSTED, &[]).await;
  let job = Uuid::new_v4();
  let in_job = |record: Record| Record {
    job_id: Some(job),
    ..record
  };
  let first = in_job(done("alice", "@@a\\Album\\1.flac"));
  let second = in_job(Record::download("alice", "@@a\\Album\\2.flac", 10, None));
  let third = in_job(Record::download("alice", "@@a\\Album\\3.flac", 10, None));
  for record in [&first, &second, &third] {
    insert(&app, record).await;
  }
  let mut stream = app
    .stream_from(
      "127.0.0.1:1",
      request("GET", "/api/v0/live?topics=transfers", None),
    )
    .await;
  assert_eq!(stream.next().await.unwrap().event, "hello");
  for record in [&third, &second] {
    let uri = format!("/api/v0/transfers/downloads/alice/{}", record.id);
    let (status, _) = app.call("DELETE", &uri, None).await;
    assert_eq!(status, 204);
  }
  let mut frames = Vec::new();
  for _ in 0..3 {
    frames.push(stream.next().await.unwrap());
  }
  let events: Vec<&str> = frames.iter().map(|f| f.event.as_str()).collect();
  assert_eq!(
    events,
    [
      "transfers.updated",
      "transfers.updated",
      "transfers.job_finished"
    ],
    "the first cancel leaves a download of the job to end"
  );
  let told = &frames[2].data;
  assert_eq!(told["id"], json!(job));
  assert_eq!(told["folder"], "@@a\\Album");
  assert_eq!(
    [
      &told["fileCount"],
      &told["succeededCount"],
      &told["canceledCount"]
    ],
    [3, 1, 2]
  );
}

#[tokio::test]
async fn a_download_removed_by_its_cancel_is_left_out_of_the_job_it_ends() {
  let app = app(TRUSTED, &[]).await;
  let job = Uuid::new_v4();
  let kept = Record {
    job_id: Some(job),
    ..done("alice", "@@a\\Album\\1.flac")
  };
  let stuck = Record {
    job_id: Some(job),
    ..Record::download("alice", "@@a\\Album\\2.flac", 10, None)
  };
  insert(&app, &kept).await;
  insert(&app, &stuck).await;
  let mut stream = app
    .stream_from(
      "127.0.0.1:1",
      request("GET", "/api/v0/live?topics=transfers", None),
    )
    .await;
  assert_eq!(stream.next().await.unwrap().event, "hello");
  let uri = format!("/api/v0/transfers/downloads/alice/{}?remove=true", stuck.id);
  let (status, _) = app.call("DELETE", &uri, None).await;
  assert_eq!(status, 204);
  let mut frames = Vec::new();
  for _ in 0..3 {
    frames.push(stream.next().await.unwrap());
  }
  let events: Vec<&str> = frames.iter().map(|f| f.event.as_str()).collect();
  assert_eq!(
    events,
    [
      "transfers.updated",
      "transfers.removed",
      "transfers.job_finished"
    ]
  );
  let told = &frames[2].data;
  assert_eq!(
    [
      &told["fileCount"],
      &told["succeededCount"],
      &told["canceledCount"]
    ],
    [1, 1, 0],
    "the removed download is not counted"
  );
}
