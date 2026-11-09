//! The download calls of the API against the in-memory daemon, which is not connected: finding
//! a download by its peer and id, canceling and removing one no task runs, a bulk cancel, a
//! finished download's file served from where it was moved, the removals and updates
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
  let mut record = Record::download(username, filename, 3);
  record.state = TransferStates::Completed | TransferStates::Succeeded;
  record.ended_at = Some(chrono::Utc::now());
  record
}

#[tokio::test]
async fn a_download_is_found_by_its_peer_and_id_with_its_history() {
  let app = app(TRUSTED, &[]).await;
  let record = Record::download("alice", "@@abcde\\Music\\01.flac", 10);
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
async fn canceling_a_download_no_task_runs_ends_its_wait_and_can_remove_it() {
  let app = app(TRUSTED, &[]).await;
  let mut waiting = Record::download("alice", "a.flac", 10);
  waiting.next_attempt_at = Some(chrono::Utc::now());
  waiting.place_in_queue = Some(4);
  insert(&app, &waiting).await;
  let id = waiting.id;
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
  assert_eq!((ended.next_attempt_at, ended.place_in_queue), (None, None));
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
async fn a_bulk_cancel_ends_the_running_download_and_fails_the_ended_and_unknown_ones() {
  let app = app(TRUSTED, &[]).await;
  let finished = done("alice", "a.flac");
  let waiting = Record::download("alice", "b.flac", 10);
  insert(&app, &finished).await;
  insert(&app, &waiting).await;
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
  let stuck = Record::download("alice", "b.flac", 10);
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
async fn a_job_is_told_once_by_the_cancel_that_ends_it_and_leaves_out_a_removed_download() {
  let app = app(TRUSTED, &[]).await;
  let job = Uuid::new_v4();
  let in_job = |record: Record| Record {
    job_id: Some(job),
    ..record
  };
  let first = in_job(done("alice", "@@a\\Album\\1.flac"));
  let second = in_job(Record::download("alice", "@@a\\Album\\2.flac", 10));
  let third = in_job(Record::download("alice", "@@a\\Album\\3.flac", 10));
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
  let canceled = format!("/api/v0/transfers/downloads/alice/{}", third.id);
  let removed = format!(
    "/api/v0/transfers/downloads/alice/{}?remove=true",
    second.id
  );
  for uri in [canceled, removed] {
    let (status, _) = app.call("DELETE", &uri, None).await;
    assert_eq!(status, 204);
  }
  let mut frames = Vec::new();
  for _ in 0..4 {
    frames.push(stream.next().await.unwrap());
  }
  let events: Vec<&str> = frames.iter().map(|f| f.event.as_str()).collect();
  assert_eq!(
    events,
    [
      "transfers.updated",
      "transfers.updated",
      "transfers.removed",
      "transfers.job_finished"
    ],
    "the first cancel leaves a download of the job to end"
  );
  let told = &frames[3].data;
  assert_eq!(told["id"], json!(job));
  assert_eq!(told["folder"], "@@a\\Album");
  assert_eq!(
    [
      &told["fileCount"],
      &told["succeededCount"],
      &told["canceledCount"]
    ],
    [2, 1, 1],
    "the removed download is not counted"
  );
}

/// Lists the download job `job` in the jobs list, as an enqueue does.
async fn listed_job(app: &TestApp, job: Uuid, name: &str) {
  let new = crate::jobs::NewJob {
    id: job,
    ..crate::jobs::NewJob::running(
      api_types::jobs::JobKind::Download,
      name,
      api_types::jobs::JobStarter::you(),
    )
  };
  app.state.jobs.create(new).await.unwrap();
}

#[tokio::test]
async fn a_paused_download_holds_its_job_paused_and_stays_paused_across_a_stop() {
  let app = app(TRUSTED, &[]).await;
  let job = Uuid::new_v4();
  listed_job(&app, job, "Album from alice").await;
  let mut waiting = Record::download("alice", "@@a\\Album\\1.flac", 10);
  waiting.job_id = Some(job);
  insert(&app, &waiting).await;
  let pause = json!({ "ids": [waiting.id], "action": "pause" });
  let (status, answer) = app
    .call("POST", "/api/v0/transfers/downloads/bulk", Some(pause))
    .await;
  assert_eq!(status, 200, "{answer}");
  assert_eq!(answer["ok"][0], waiting.id.to_string());
  let paused = read(&app, waiting.id).await;
  assert!(paused.paused);
  assert_eq!(
    paused.state,
    TransferStates::Queued | TransferStates::Locally
  );
  let listed = app.state.jobs.get(job).await.unwrap().unwrap();
  assert_eq!(listed.state, api_types::jobs::JobState::Paused);
  assert_eq!(listed.counts.paused, 1);

  crate::transfers::cleanup::end_unfinished(&app.state.transfers)
    .await
    .unwrap();
  let kept = read(&app, waiting.id).await;
  assert!(
    kept.paused && kept.ended_at.is_none(),
    "a stop leaves it paused"
  );

  let again = json!({ "ids": [waiting.id], "action": "pause" });
  let (_, answer) = app
    .call("POST", "/api/v0/transfers/downloads/bulk", Some(again))
    .await;
  assert_eq!(
    answer["failed"][0]["error"],
    "the download is paused already"
  );
  let resume = json!({ "ids": [waiting.id], "action": "resume" });
  let (_, answer) = app
    .call("POST", "/api/v0/transfers/downloads/bulk", Some(resume))
    .await;
  assert_eq!(answer["ok"][0], waiting.id.to_string(), "{answer}");
  assert!(!read(&app, waiting.id).await.paused);
}

#[tokio::test]
async fn a_download_job_ends_in_the_list_with_its_files_named_under_its_folder() {
  let app = app(TRUSTED, &[]).await;
  let job = Uuid::new_v4();
  listed_job(&app, job, "Album from alice").await;
  let mut arrived = done("alice", "@@a\\Album\\CD1\\1.flac");
  arrived.job_id = Some(job);
  let mut waiting = Record::download("alice", "@@a\\Album\\CD2\\1.flac", 10);
  waiting.job_id = Some(job);
  insert(&app, &arrived).await;
  insert(&app, &waiting).await;
  let uri = format!("/api/v0/transfers/downloads/alice/{}", waiting.id);
  let (status, _) = app.call("DELETE", &uri, None).await;
  assert_eq!(status, 204);
  let (_, detail) = app.call("GET", &format!("/api/v0/jobs/{job}"), None).await;
  assert_eq!(detail["job"]["state"], "ended", "{detail}");
  assert_eq!(
    detail["job"]["ending"], "succeeded",
    "a canceled file was the user's choice"
  );
  let parts: Vec<(&str, &str)> = detail["parts"]
    .as_array()
    .unwrap()
    .iter()
    .map(|p| (p["name"].as_str().unwrap(), p["state"].as_str().unwrap()))
    .collect();
  assert_eq!(
    parts,
    [("CD1\\1.flac", "succeeded"), ("CD2\\1.flac", "canceled")]
  );
}

#[tokio::test]
async fn a_folder_download_skips_the_files_the_filters_match_before_asking_its_user() {
  let app = app(TRUSTED, &[]).await;
  let group = |names: &[&str]| super::Group {
    username: "alice".into(),
    files: names
      .iter()
      .map(|name| super::FileRequest {
        filename: format!("@@a\\Album\\{name}"),
        size: 1,
      })
      .collect(),
    destination: None,
    search_id: None,
  };
  let kept = ["Thumbs.db.flac", "01 desktop.ini.mp3", "exe.flac"];
  let mut groups = vec![group(&["setup.EXE", "Thumbs.db", ".DS_Store"])];
  groups.extend(kept.map(|name| group(&[name])));
  let downloads = &app.state.downloads;
  let mut results = downloads
    .enqueue(Uuid::new_v4(), groups, true)
    .await
    .into_iter();
  let enqueued = results
    .next()
    .unwrap()
    .expect("nothing left to ask alice for");
  assert!(enqueued.enqueued.is_empty());
  let skipped: Vec<&str> = enqueued.failed.iter().map(|f| f.message.as_str()).collect();
  assert_eq!(skipped, ["Skipped: Filtered"; 3]);
  // a file the filters pass goes on to ask alice, which the missing server refuses
  for (name, result) in kept.iter().zip(results) {
    let refused = result.err().unwrap_or_else(|| panic!("{name} is skipped"));
    assert!(refused.is_unavailable(), "{name} is downloaded");
  }
}

#[tokio::test]
async fn the_gauges_count_the_downloads_waiting_here_but_not_the_paused_ones() {
  let app = app(TRUSTED, &[]).await;
  let with = |username: &str, state: TransferStates, paused: bool| {
    let mut record = Record::download(username, &format!("{username}.flac"), 100);
    record.state = state;
    record.paused = paused;
    record
  };
  for record in [
    with(
      "alice",
      TransferStates::Queued | TransferStates::Locally,
      false,
    ),
    with(
      "bob",
      TransferStates::Queued | TransferStates::Remotely,
      false,
    ),
    with(
      "carol",
      TransferStates::Queued | TransferStates::Locally,
      true,
    ),
    with("dave", TransferStates::InProgress, false),
    done("erin", "e.flac"),
  ] {
    insert(&app, &record).await;
  }
  let (queued, in_progress) = app.state.downloads.tallies().await.unwrap();
  assert_eq!(queued, (2, 2, 200));
  assert_eq!(in_progress, (1, 1, 100));
}

/// Reads the download `id` until `done` holds for it, for ten seconds at most.
async fn read_until(app: &TestApp, id: Uuid, done: impl Fn(&Record) -> bool) -> Record {
  for _ in 0..1000 {
    let record = read(app, id).await;
    if done(&record) {
      return record;
    }
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
  }
  panic!("the download never came to the state asked for");
}

#[tokio::test]
async fn a_download_paused_while_its_task_runs_waits_here_without_failing() {
  let config = format!("[transfers.download.retry]\ndelay = \"1h\"\nmax_delay = \"1h\"\n{TRUSTED}");
  let app = app(&config, &[]).await;
  let failed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
  let counted = failed.clone();
  app.state.events.subscribe(
    "test",
    Some(api_types::events::EventType::DownloadFileFailed),
    move |_| {
      counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
      async { Ok(()) }
    },
  );
  let mut held = Record::download("alice", "@@a\\Album\\1.flac", 10);
  held.paused = true;
  insert(&app, &held).await;
  let id = held.id;
  // resumed, its task tries the peer, which cannot be reached, and waits to try again
  let resume = json!({ "ids": [id], "action": "resume" });
  app
    .call("POST", "/api/v0/transfers/downloads/bulk", Some(resume))
    .await;
  read_until(&app, id, |r| r.next_attempt_at.is_some()).await;

  let pause = json!({ "ids": [id], "action": "pause" });
  let (_, answer) = app
    .call("POST", "/api/v0/transfers/downloads/bulk", Some(pause))
    .await;
  assert_eq!(answer["ok"][0], id.to_string(), "{answer}");
  let paused = read_until(&app, id, |r| r.paused).await;
  assert_eq!(
    paused.state,
    TransferStates::Queued | TransferStates::Locally
  );
  assert_eq!(
    (paused.exception, paused.next_attempt_at, paused.ended_at),
    (None, None, None)
  );
  assert_eq!(
    app
      .state
      .telemetry
      .metrics()
      .transfers()
      .downloads
      .completed_failed
      .get(),
    0.0,
    "a pause is no failure"
  );
  tokio::time::sleep(std::time::Duration::from_millis(100)).await;
  assert_eq!(failed.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_resume_sent_while_the_download_stops_to_be_paused_waits_and_resumes_it() {
  let config = format!("[transfers.download.retry]\ndelay = \"1h\"\nmax_delay = \"1h\"\n{TRUSTED}");
  let app = app(&config, &[]).await;
  let mut held = Record::download("alice", "@@a\\Album\\1.flac", 10);
  held.paused = true;
  insert(&app, &held).await;
  let id = held.id;
  let bulk = |action: &str| json!({ "ids": [id], "action": action });
  app
    .call("POST", "/api/v0/transfers/downloads/bulk", Some(bulk("resume")))
    .await;
  read_until(&app, id, |r| r.next_attempt_at.is_some()).await;
  // the pause answers once the task is told to stop, before the task has written it paused
  app
    .call("POST", "/api/v0/transfers/downloads/bulk", Some(bulk("pause")))
    .await;
  let (_, answer) = app
    .call("POST", "/api/v0/transfers/downloads/bulk", Some(bulk("resume")))
    .await;
  assert_eq!(answer["ok"][0], id.to_string(), "{answer}");
  assert!(!read(&app, id).await.paused);
}

#[tokio::test]
async fn a_download_job_that_never_had_a_download_ends_on_cancel_and_after_a_restart() {
  let app = app(TRUSTED, &[]).await;
  let (canceled, stranded) = (Uuid::new_v4(), Uuid::new_v4());
  listed_job(&app, canceled, "Album from alice").await;
  listed_job(&app, stranded, "Album from bob").await;
  let (status, _) = app
    .call("POST", &format!("/api/v0/jobs/{canceled}/cancel"), None)
    .await;
  assert_eq!(status, 204);
  let job = app.state.jobs.get(canceled).await.unwrap().unwrap();
  assert_eq!(job.ending, Some(api_types::jobs::JobEnding::Canceled));

  app.state.downloads.resume().await;
  let job = app.state.jobs.get(stranded).await.unwrap().unwrap();
  assert_eq!(job.ending, Some(api_types::jobs::JobEnding::Failed));
  assert_eq!(job.error.as_deref(), Some("Nothing was queued"));
}

#[tokio::test]
async fn a_job_refused_for_want_of_the_server_says_so_in_words() {
  let app = app(TRUSTED, &[]).await;
  let request = json!({
    "groups": [{ "username": "alice", "files": [{ "filename": "@@a\\1.flac", "size": 1 }] }]
  });
  let (_, answer) = app
    .call("POST", "/api/v0/transfers/downloads/jobs", Some(request))
    .await;
  let id: Uuid = answer["id"].as_str().unwrap().parse().unwrap();
  let job = app.state.jobs.get(id).await.unwrap().unwrap();
  assert_eq!(job.ending, Some(api_types::jobs::JobEnding::Failed));
  assert_eq!(
    job.error.as_deref(),
    Some("Not connected to the Soulseek server")
  );
}
