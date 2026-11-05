//! The analysis calls against the in-memory daemon: a downloaded file analyzed by its path and
//! by its download, its report, its spectrogram and its waveform, the event and the frame raised
//! once, files outside the roots refused, unreadable files reported rather than failed, the
//! user's judgment kept across a restart, a batch larger than the queue run whole, and a file
//! uploaded for one analysis leaving nothing behind.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use api_types::events::EventType;
use api_types::transfers::TransferStates;
use axum::body::Body;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use super::analyzer::fixtures::{flac, music, pink, write_flac};
use super::jobs::QUEUE_LIMIT;
use crate::testing::{TRUSTED, TestApp, app, request, with};
use crate::transfers::{Record, store as transfers};

/// One analysis at a time, so a test knows which files wait.
const ONE_WORKER: &str = "[analysis]\nworkers = 1\n";

/// The analysis route of the file `relative` under the downloads directory.
fn route(relative: &str) -> String {
  format!(
    "/api/v0/files/downloads/files/{}/analysis",
    URL_SAFE_NO_PAD.encode(relative)
  )
}

/// Writes a lossless FLAC of generated music at `relative` under the downloads directory. The
/// music is made and encoded once, and every file is a copy of it.
fn lossless(app: &TestApp, relative: &str) {
  static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
  let bytes = BYTES.get_or_init(|| flac(&music(1, 44_100, pink), 16, false));
  let path = app.state.paths.downloads.join(relative);
  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  std::fs::write(path, bytes).unwrap();
}

/// The JSON header of a binary answer and the bytes after it.
fn framed(body: &[u8]) -> (Value, &[u8]) {
  let length = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
  let header = serde_json::from_slice(&body[4..4 + length]).unwrap();
  (header, &body[4 + length..])
}

/// How many files the analysis directory holds.
fn kept(app: &TestApp) -> usize {
  std::fs::read_dir(app.state.paths.data.join("analysis"))
    .unwrap()
    .count()
}

/// Waits until no analysis is queued or running.
async fn settled(app: &TestApp) {
  for _ in 0..1200 {
    let (status, queue) = app.call("GET", "/api/v0/analysis/jobs", None).await;
    assert_eq!(status, 200);
    if queue["jobs"].as_array().unwrap().is_empty() && queue["backlog"] == 0 {
      return;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
  panic!("the analyses did not finish");
}

/// Counts the `FileAnalysisComplete` events raised.
fn count_events(app: &TestApp) -> Arc<AtomicUsize> {
  let count = Arc::new(AtomicUsize::new(0));
  let counted = count.clone();
  app.state.events.subscribe(
    "analysis-test",
    Some(EventType::FileAnalysisComplete),
    move |_| {
      counted.fetch_add(1, Ordering::SeqCst);
      async { Ok(()) }
    },
  );
  count
}

#[tokio::test]
async fn a_download_is_analyzed_reported_drawn_and_announced_once() {
  let app = app(TRUSTED, &[]).await;
  lossless(&app, "Album/01.flac");
  let mut record = Record::download("alice", "@@share\\Album\\01.flac", 3);
  record.state = TransferStates::Completed | TransferStates::Succeeded;
  let local = app.state.paths.downloads.join("Album/01.flac");
  record.local_path = Some(local.to_string_lossy().into_owned());
  let id = record.id;
  let inserted = record.clone();
  app
    .state
    .transfers
    .write(move |conn| transfers::insert(conn, &inserted))
    .await
    .unwrap();
  let events = count_events(&app);
  let mut stream = app
    .stream_from(
      "127.0.0.1:1",
      request("GET", "/api/v0/live?topics=transfers", None),
    )
    .await;
  assert_eq!(stream.next().await.unwrap().event, "hello");

  let (status, created) = app.call("POST", &route("Album/01.flac"), None).await;
  assert_eq!(status, 202, "{created}");
  assert!(created["jobId"].is_string());
  let (status, _) = app.call("POST", &route("Album/01.flac"), None).await;
  assert_eq!(status, 409, "queued, running or analyzed already");
  let frames = stream.until("files.analysis").await;
  let queued = frames
    .iter()
    .find(|frame| frame.event == "files.analysis_queued")
    .unwrap_or_else(|| panic!("no frame said the file was queued: {frames:?}"));
  assert_eq!(queued.data["id"], created["jobId"]);
  assert_eq!(queued.data["relativePath"], "Album/01.flac");
  let frame = &frames.last().unwrap().data;
  assert_eq!(frame["transferId"], json!(id));
  assert_eq!(frame["verdict"], "consistent");
  settled(&app).await;

  let (status, report) = app.call("GET", &route("Album/01.flac"), None).await;
  assert_eq!(status, 200, "{report}");
  assert_eq!(report["verdict"], "consistent");
  assert_eq!(report["path"], local.to_string_lossy().as_ref());
  // named as the files endpoints and the transfers name it, for a client to find its rows by
  assert_eq!(report["root"], "downloads");
  assert_eq!(report["relativePath"], "Album/01.flac");
  assert_eq!(report["transferId"], json!(id));
  assert_eq!(report["claimed"]["codec"], "flac");
  assert_eq!(report["claimed"]["bitDepth"], 16);
  assert!(report["windows"].as_array().unwrap().len() >= 30);
  assert_eq!(report["override"], Value::Null);
  let url = report["spectrogramUrl"].as_str().unwrap();
  assert_eq!(url, format!("{}/spectrogram", route("Album/01.flac")));
  let layout = &report["spectrogram"];
  assert_eq!(layout["bins"], 512);
  assert_eq!(
    layout["scale"], "log",
    "the option's until a viewer picks one"
  );
  let by_download = format!("/api/v0/transfers/downloads/alice/{id}/analysis?windows=false");
  let (status, same) = app.call("GET", &by_download, None).await;
  assert_eq!(status, 200);
  assert_eq!(same["windows"], Value::Null);
  assert_eq!(same["analyzedAt"], report["analyzedAt"]);
  let (_, listed) = app.call("GET", "/api/v0/analysis", None).await;
  assert_eq!(listed[0]["relativePath"], "Album/01.flac");
  assert_eq!(listed[0]["transferId"], json!(id));

  let overview = app.send(request("GET", url, None)).await;
  assert_eq!(overview.status, 200);
  assert_eq!(overview.headers["content-type"], "application/octet-stream");
  let (_, columns) = framed(&overview.body);
  assert!(columns.iter().any(|b| *b > 128), "music is loud somewhere");
  // a viewer that has it already is told so
  let tag = overview.headers["etag"].to_str().unwrap().to_string();
  let mut again = request("GET", url, None);
  again
    .headers_mut()
    .insert("if-none-match", tag.parse().unwrap());
  let again = app.send(again).await;
  assert_eq!(again.status, 304);
  assert!(again.body.is_empty());
  let tile = app
    .send(request("GET", &format!("{url}?tile=0"), None))
    .await;
  assert_eq!(tile.status, 200);
  assert_ne!(tile.headers["etag"].to_str().unwrap(), tag);
  let past = app
    .send(request("GET", &format!("{url}?tile=9"), None))
    .await;
  assert_eq!(past.status, 404);
  let waveform = format!("/api/v0/transfers/downloads/alice/{id}/analysis/waveform");
  let waveform = app.send(request("GET", &waveform, None)).await;
  assert_eq!(waveform.status, 200);
  let (header, peaks) = framed(&waveform.body);
  assert_eq!(peaks.len(), header["peaks"].as_u64().unwrap() as usize * 2);
  let spectra = format!("{}/spectra", route("Album/01.flac"));
  let spectra = app.send(request("GET", &spectra, None)).await;
  let (header, _) = framed(&spectra.body);
  assert_eq!(
    header["windows"],
    report["windows"].as_array().unwrap().len()
  );

  assert_eq!(events.load(Ordering::SeqCst), 1);
  let (status, _) = app.call("POST", &route("Album/01.flac"), None).await;
  assert_eq!(status, 409, "the report is current");
  let (status, _) = app
    .call(
      "POST",
      &format!("/api/v0/transfers/downloads/alice/{id}/analysis?force=true"),
      None,
    )
    .await;
  assert_eq!(status, 202);
  settled(&app).await;
  // the event subscriber runs on its own task
  for _ in 0..100 {
    if events.load(Ordering::SeqCst) == 2 {
      break;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
  assert_eq!(events.load(Ordering::SeqCst), 2, "once per analysis");
}

#[cfg(unix)]
#[tokio::test]
async fn a_file_outside_the_roots_is_refused() {
  let app = app(TRUSTED, &[]).await;
  let elsewhere = tempfile::tempdir().unwrap();
  let outside = elsewhere.path().join("x.flac");
  write_flac(&music(1, 44_100, pink), 16, false, &outside);
  let downloads = &app.state.paths.downloads;
  std::fs::create_dir_all(downloads).unwrap();
  std::os::unix::fs::symlink(&outside, downloads.join("link.flac")).unwrap();
  let (status, body) = app.call("POST", &route("link.flac"), None).await;
  assert_eq!(status, 403, "{body}");
  let (status, _) = app.call("GET", &route("link.flac"), None).await;
  assert_eq!(status, 403);
  let escape = format!(
    "/api/v0/files/downloads/files/{}/analysis",
    URL_SAFE_NO_PAD.encode("../x.flac")
  );
  let (status, _) = app.call("POST", &escape, None).await;
  assert_eq!(status, 400);
  let (status, _) = app.call("POST", &route("missing.flac"), None).await;
  assert_eq!(status, 404);
}

#[tokio::test]
async fn unreadable_files_are_reported_inconclusive_not_failed() {
  let app = app(TRUSTED, &[]).await;
  let downloads = app.state.paths.downloads.clone();
  std::fs::create_dir_all(&downloads).unwrap();
  lossless(&app, "whole.flac");
  let whole = std::fs::read(downloads.join("whole.flac")).unwrap();
  std::fs::write(downloads.join("cut.flac"), &whole[..whole.len() / 50]).unwrap();
  std::fs::write(downloads.join("noise.flac"), b"this is not audio at all").unwrap();
  std::fs::write(downloads.join("empty.wav"), b"").unwrap();
  for name in ["cut.flac", "noise.flac", "empty.wav"] {
    let (status, _) = app.call("POST", &route(name), None).await;
    assert_eq!(status, 202, "{name}");
  }
  settled(&app).await;
  for name in ["cut.flac", "noise.flac", "empty.wav"] {
    let (status, report) = app.call("GET", &route(name), None).await;
    assert_eq!(status, 200, "{name}: {report}");
    assert_eq!(report["verdict"], "inconclusive", "{name}");
    assert!(
      !report["limitations"].as_array().unwrap().is_empty(),
      "{name}: {report}"
    );
  }
  let (_, noise) = app.call("GET", &route("noise.flac"), None).await;
  assert_eq!(noise["claimed"]["codec"], "unknown");
  assert_eq!(noise["spectrogram"], Value::Null);
  let pictures = format!("{}/spectrogram", route("noise.flac"));
  let (status, _) = app.call("GET", &pictures, None).await;
  assert_eq!(status, 404);
}

#[tokio::test]
async fn the_judgment_is_kept_across_a_restart_and_beside_a_new_analysis() {
  let app = app(TRUSTED, &[]).await;
  lossless(&app, "a.flac");
  let judgment = format!("{}/override", route("a.flac"));
  let fake = json!({ "verdict": "fake", "note": "sounds like 128k" });
  let (status, _) = app.call("PUT", &judgment, Some(fake.clone())).await;
  assert_eq!(status, 404, "nothing to judge before an analysis");
  app.call("POST", &route("a.flac"), None).await;
  settled(&app).await;
  let (status, _) = app.call("PUT", &judgment, Some(fake)).await;
  assert_eq!(status, 204);

  let app = app.restarted(&[]).await;
  let (_, report) = app.call("GET", &route("a.flac"), None).await;
  assert_eq!(
    report["verdict"], "consistent",
    "the reading stays beside it"
  );
  assert_eq!(report["override"]["verdict"], "fake");
  assert_eq!(report["override"]["note"], "sounds like 128k");
  assert!(report["override"]["by"].is_string());
  let (_, listed) = app.call("GET", "/api/v0/analysis", None).await;
  assert_eq!(listed[0]["override"]["verdict"], "fake");
  assert_eq!(listed[0]["windows"], Value::Null);

  app
    .call("POST", &format!("{}?force=true", route("a.flac")), None)
    .await;
  settled(&app).await;
  let (_, report) = app.call("GET", &route("a.flac"), None).await;
  assert_eq!(
    report["override"]["verdict"], "fake",
    "a new analysis keeps it"
  );

  let (status, _) = app.call("DELETE", &judgment, None).await;
  assert_eq!(status, 204);
  let (_, report) = app.call("GET", &route("a.flac"), None).await;
  assert_eq!(report["override"], Value::Null);
}

#[tokio::test]
async fn a_report_and_its_judgment_outlive_the_file() {
  let app = app(TRUSTED, &[]).await;
  lossless(&app, "Album/02.flac");
  let mut record = Record::download("alice", "@@share\\Album\\02.flac", 3);
  record.state = TransferStates::Completed | TransferStates::Succeeded;
  let local = app.state.paths.downloads.join("Album/02.flac");
  record.local_path = Some(local.to_string_lossy().into_owned());
  let id = record.id;
  app
    .state
    .transfers
    .write(move |conn| transfers::insert(conn, &record))
    .await
    .unwrap();
  let by_download = format!("/api/v0/transfers/downloads/alice/{id}/analysis");
  app.call("POST", &by_download, None).await;
  settled(&app).await;
  let fake = json!({ "verdict": "fake", "note": "a transcode" });
  let (status, _) = app
    .call("PUT", &format!("{by_download}/override"), Some(fake))
    .await;
  assert_eq!(status, 204);

  std::fs::remove_file(&local).unwrap();
  let (status, report) = app.call("GET", &by_download, None).await;
  assert_eq!(status, 200, "{report}");
  assert_eq!(report["override"]["verdict"], "fake");
  let limitations = report["limitations"].as_array().unwrap();
  assert!(
    limitations.contains(&json!("The file is no longer there.")),
    "{report}"
  );
  let (status, _) = app.call("GET", &route("Album/02.flac"), None).await;
  assert_eq!(status, 200, "by its path too");
  let image = app
    .send(request("GET", &format!("{by_download}/spectrogram"), None))
    .await;
  assert_eq!(image.status, 200);
  let legit = json!({ "verdict": "legit" });
  let (status, _) = app
    .call("PUT", &format!("{by_download}/override"), Some(legit))
    .await;
  assert_eq!(status, 204);
  let (status, _) = app
    .call("DELETE", &format!("{by_download}/override"), None)
    .await;
  assert_eq!(status, 204);
  let (status, _) = app
    .call("POST", &format!("{by_download}?force=true"), None)
    .await;
  assert_eq!(status, 404, "a file that is gone cannot be analyzed again");
}

#[tokio::test]
async fn spectrograms_beyond_the_cache_limit_go_least_recently_viewed_first() {
  let app = app(&format!("remote_configuration = true\n{TRUSTED}"), &[]).await;
  for name in ["a.flac", "b.flac", "c.flac"] {
    lossless(&app, name);
  }
  for name in ["a.flac", "b.flac"] {
    app.call("POST", &route(name), None).await;
    settled(&app).await;
  }
  assert_eq!(kept(&app), 2);
  let spectrogram = |name: &str| format!("{}/spectrogram", route(name));
  assert_eq!(
    app
      .send(request("GET", &spectrogram("a.flac"), None))
      .await
      .status,
    200
  );

  // lowering the limit removes at once, and the one viewed last stays
  let lowered = json!({ "set": { "analysis.cache_max_bytes": "1B" } });
  let (status, body) = app.call("PATCH", "/api/v0/options", Some(lowered)).await;
  assert_eq!(status, 200, "{body}");
  for _ in 0..200 {
    if kept(&app) == 1 {
      break;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
  assert_eq!(kept(&app), 1, "the most recently viewed kept");
  let (status, gone) = app.call("GET", &spectrogram("b.flac"), None).await;
  assert_eq!(status, 404, "{gone}");

  // a new analysis over the limit removes the least recently viewed
  app.call("POST", &route("c.flac"), None).await;
  settled(&app).await;
  assert_eq!(kept(&app), 1, "the newest report's file");
  assert_eq!(app.call("GET", &spectrogram("a.flac"), None).await.0, 404);
  assert_eq!(
    app
      .send(request("GET", &spectrogram("c.flac"), None))
      .await
      .status,
    200
  );
}

#[tokio::test]
async fn a_queued_analysis_can_be_canceled_and_a_window_read() {
  let app = app(&format!("{TRUSTED}{ONE_WORKER}"), &[]).await;
  for name in ["a.flac", "b.flac", "c.flac"] {
    lossless(&app, name);
  }
  let mut ids = Vec::new();
  for name in ["a.flac", "b.flac", "c.flac"] {
    let (status, created) = app.call("POST", &route(name), None).await;
    assert_eq!(status, 202);
    ids.push(created["jobId"].as_str().unwrap().to_string());
  }
  // one worker: the last is still waiting
  let (status, _) = app
    .call("DELETE", &format!("/api/v0/analysis/jobs/{}", ids[2]), None)
    .await;
  assert_eq!(status, 204);
  let (_, queue) = app.call("GET", "/api/v0/analysis/jobs", None).await;
  let jobs = &queue["jobs"];
  let listed: Vec<&str> = jobs
    .as_array()
    .unwrap()
    .iter()
    .map(|j| j["id"].as_str().unwrap())
    .collect();
  assert!(!listed.contains(&ids[2].as_str()), "{jobs}");
  // a job names its file as the request did, so any client can find the file's job
  let waiting = jobs
    .as_array()
    .unwrap()
    .iter()
    .find(|j| j["id"] == ids[1].as_str())
    .unwrap();
  assert_eq!(waiting["root"], "downloads");
  assert_eq!(waiting["relativePath"], "b.flac");
  let unknown = format!("/api/v0/analysis/jobs/{}", uuid::Uuid::new_v4());
  assert_eq!(app.call("DELETE", &unknown, None).await.0, 404);
  settled(&app).await;
  let (status, _) = app.call("GET", &route("c.flac"), None).await;
  assert_eq!(status, 404, "the canceled file was not analyzed");

  let (status, window) = app
    .call("GET", &format!("{}/windows/3", route("a.flac")), None)
    .await;
  assert_eq!(status, 200, "{window}");
  assert_eq!(window["t"], 3.0);
  assert_eq!(window["hz"].as_array().unwrap().len(), 512);
  assert_eq!(window["db"].as_array().unwrap().len(), 512);
  // linear bins, lowest first, to the top of the band
  let hz = |i: usize| window["hz"][i].as_f64().unwrap();
  assert!((hz(0) - 21.5).abs() < 0.1, "{}", hz(0));
  assert!((hz(511) - 22_028.5).abs() < 0.1, "{}", hz(511));
  let (status, _) = app
    .call("GET", &format!("{}/windows/999", route("a.flac")), None)
    .await;
  assert_eq!(status, 404);
}

#[tokio::test]
async fn a_report_from_an_older_version_lists_as_stale_keeps_its_judgment_and_is_replaced() {
  let app = app(TRUSTED, &[]).await;
  lossless(&app, "a.flac");
  app.call("POST", &route("a.flac"), None).await;
  settled(&app).await;
  let judgment = json!({ "verdict": "fake", "note": "sounds like 128k" });
  let judged = format!("{}/override", route("a.flac"));
  assert_eq!(app.call("PUT", &judged, Some(judgment)).await.0, 204);
  // the report as an older version stored it, numbered as an older shape
  let stored = rusqlite::Connection::open(app.state.paths.data.join("analysis.db")).unwrap();
  stored
    .execute("UPDATE reports SET format = format - 1", [])
    .unwrap();

  let (status, listed) = app
    .call("GET", "/api/v0/analysis?offset=0&limit=500", None)
    .await;
  assert_eq!(status, 200, "{listed}");
  let entry = &listed[0];
  assert_eq!(entry["stale"], true);
  assert_eq!(entry["relativePath"], "a.flac");
  assert_eq!(entry["override"]["note"], "sounds like 128k");
  let (status, report) = app.call("GET", &route("a.flac"), None).await;
  assert_eq!(status, 200, "{report}");
  assert_eq!(report["stale"], true);
  assert_eq!(report["override"]["verdict"], "fake");
  assert_eq!(report["spectrogram"], Value::Null);
  let pictures = format!("{}/spectrogram", route("a.flac"));
  assert_eq!(app.send(request("GET", &pictures, None)).await.status, 404);

  // analyzing again needs no force: a stale report is no reading of the file
  let (status, created) = app.call("POST", &route("a.flac"), None).await;
  assert_eq!(status, 202, "{created}");
  settled(&app).await;
  let (_, report) = app.call("GET", &route("a.flac"), None).await;
  assert_eq!(report["stale"], false);
  assert!(report["spectrogram"]["durationSeconds"].is_number());
  assert_eq!(
    report["override"]["note"], "sounds like 128k",
    "the judgment stays"
  );
}

#[tokio::test]
async fn a_batch_past_the_queue_limit_joins_the_queue_as_it_drains() {
  let app = app(&format!("{TRUSTED}{ONE_WORKER}"), &[]).await;
  let names: Vec<String> = (0..QUEUE_LIMIT + 20)
    .map(|n| format!("batch/{n:02}.flac"))
    .collect();
  for name in &names {
    lossless(&app, name);
  }
  let file = |path: &str| json!({ "kind": "file", "root": "downloads", "path": path });
  let files: Vec<Value> = [file("../escape.flac"), file("batch/missing.flac")]
    .into_iter()
    .chain(names.iter().map(|name| file(name)))
    .collect();
  let (status, answer) = app
    .call(
      "POST",
      "/api/v0/analysis/batch",
      Some(json!({ "files": files })),
    )
    .await;
  assert_eq!(status, 202, "{answer}");
  let count = |key: &str| answer[key].as_u64().unwrap();
  assert_eq!(
    count("queued") + count("following"),
    names.len() as u64,
    "{answer}"
  );
  assert!(
    count("following") > 0,
    "the queue took every file: {answer}"
  );
  let refused: Vec<u64> = answer["refused"]
    .as_array()
    .unwrap()
    .iter()
    .map(|refusal| refusal["index"].as_u64().unwrap())
    .collect();
  assert_eq!(refused, [0, 1], "{answer}");

  settled(&app).await;
  for name in &names {
    let (status, report) = app.call("GET", &route(name), None).await;
    assert_eq!(status, 200, "{name} was not analyzed: {report}");
  }
}

#[tokio::test]
async fn a_batchs_backlog_shows_in_the_queue_and_goes_when_it_is_cleared() {
  let app = app(&format!("{TRUSTED}{ONE_WORKER}"), &[]).await;
  let names: Vec<String> = (0..QUEUE_LIMIT + 20)
    .map(|n| format!("batch/{n:02}.flac"))
    .collect();
  for name in &names {
    lossless(&app, name);
  }
  let files: Vec<Value> = names
    .iter()
    .map(|path| json!({ "kind": "file", "root": "downloads", "path": path }))
    .collect();
  let (status, answer) = app
    .call(
      "POST",
      "/api/v0/analysis/batch",
      Some(json!({ "files": files })),
    )
    .await;
  assert_eq!(status, 202, "{answer}");
  let (_, queue) = app.call("GET", "/api/v0/analysis/jobs", None).await;
  // the worker may have finished a file or two since, each making room for one
  assert!(queue["backlog"].as_u64().unwrap() > 10, "{queue}");

  let (status, _) = app.call("DELETE", "/api/v0/analysis/jobs", None).await;
  assert_eq!(status, 204);
  let (_, queue) = app.call("GET", "/api/v0/analysis/jobs", None).await;
  assert_eq!(queue["backlog"], 0, "{queue}");
  let jobs = queue["jobs"].as_array().unwrap();
  assert!(
    jobs.iter().all(|job| job["state"] == "running"),
    "only the running analysis goes on: {queue}"
  );
  settled(&app).await;
  let (status, _) = app.call("GET", &route(names.last().unwrap()), None).await;
  assert_eq!(status, 404, "a file of the backlog was not analyzed");
}

/// Uploads `body` for analysis as a file named `name`.
async fn upload(app: &TestApp, name: &str, body: Body) -> (u16, Value) {
  let mut sent = request(
    "POST",
    &format!("/api/v0/analysis/uploads?name={name}"),
    None,
  );
  *sent.body_mut() = body;
  let answer = app.send(sent).await;
  let json = serde_json::from_slice(&answer.body).unwrap_or(Value::Null);
  (answer.status.as_u16(), json)
}

/// The analysis route of an upload the daemon answered with.
fn uploaded(answer: &Value) -> String {
  temporary(answer["path"].as_str().unwrap())
}

fn temporary(relative: &str) -> String {
  format!(
    "/api/v0/files/temporary/files/{}/analysis",
    URL_SAFE_NO_PAD.encode(relative)
  )
}

/// How many uploads the cache holds.
fn uploads_held(app: &TestApp) -> usize {
  let dir = crate::analysis::uploads_dir(&app.state.paths.data);
  std::fs::read_dir(dir).map_or(0, Iterator::count)
}

/// Waits until the cache holds `count` uploads.
async fn uploads_settle(app: &TestApp, count: usize) {
  for _ in 0..200 {
    if uploads_held(app) == count {
      return;
    }
    tokio::time::sleep(Duration::from_millis(10)).await;
  }
  assert_eq!(uploads_held(app), count);
}

/// A body whose length is not known until it ends, as a chunked upload's is not.
fn chunked(bytes: Vec<u8>) -> Body {
  Body::from_stream(futures_util::stream::iter(
    bytes
      .chunks(256)
      .map(|chunk| Ok::<_, std::io::Error>(chunk.to_vec()))
      .collect::<Vec<_>>(),
  ))
}

#[tokio::test]
async fn an_upload_stops_at_its_limit_and_leaves_nothing() {
  let config = format!("{TRUSTED}[analysis]\nupload_max_bytes = \"1KB\"\n");
  let app = app(&config, &[]).await;
  // refused from its length, before any of it is read
  let (status, body) = upload(&app, "a.flac", Body::from(vec![0; 1001])).await;
  assert_eq!(status, 413, "{body}");
  // and at the byte that passes the limit when the length is not given
  let (status, body) = upload(&app, "a.flac", chunked(vec![0; 2000])).await;
  assert_eq!(status, 413, "{body}");
  uploads_settle(&app, 0).await;
  let (status, body) = upload(&app, "a.flac", chunked(vec![0; 1000])).await;
  assert_eq!(status, 201, "{body}");
  assert_eq!(body["size"], 1000);
  assert_eq!(body["root"], "temporary");
  uploads_settle(&app, 1).await;

  let (status, body) = upload(&app, "", Body::from(vec![0; 10])).await;
  assert_eq!(status, 400, "{body}");
}

#[tokio::test]
async fn an_upload_too_long_to_name_is_named_as_it_can_be_created() {
  let app = app(TRUSTED, &[]).await;
  // 270 bytes of UTF-8 before the extension
  let long = format!("{}.flac", "曲".repeat(90));
  let query: String = url::form_urlencoded::byte_serialize(long.as_bytes()).collect();
  let (status, answer) = upload(&app, &query, Body::from(vec![0; 10])).await;
  assert_eq!(status, 201, "{answer}");
  let name = answer["path"].as_str().unwrap().split('/').nth(1).unwrap();
  assert!(name.len() <= 255, "{name}");
  assert!(name.ends_with(".flac"), "{name}");
}

#[tokio::test]
async fn a_principal_uploads_one_file_at_a_time() {
  let app = app(TRUSTED, &[]).await;
  // a body that ends when its sender is dropped
  let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(1);
  let body = futures_util::stream::unfold(receiver, |mut receiver| async move {
    receiver.recv().await.map(|piece| (piece, receiver))
  });
  let first = upload(&app, "a.flac", Body::from_stream(body));
  let second = async {
    // the first upload's folder is made once it holds the slot
    uploads_settle(&app, 1).await;
    let answer = upload(&app, "b.flac", Body::from(vec![0; 10])).await;
    drop(sender);
    answer
  };
  let ((status, first), (refused, second)) = tokio::join!(first, second);
  assert_eq!(refused, 429, "{second}");
  assert_eq!(status, 201, "{first}");
  let (status, answer) = upload(&app, "c.flac", Body::from(vec![0; 10])).await;
  assert_eq!(
    status, 201,
    "the slot is free once the first ends: {answer}"
  );
}

// paused time moves on to the end of the wait whenever every task waits
#[tokio::test(start_paused = true)]
async fn an_upload_that_stops_sending_ends_and_frees_its_slot() {
  let app = app(TRUSTED, &[]).await;
  let stalled = futures_util::stream::pending::<Result<Vec<u8>, std::io::Error>>();
  let (status, answer) = upload(&app, "a.flac", Body::from_stream(stalled)).await;
  assert_eq!(status, 408, "{answer}");
  let (status, answer) = upload(&app, "b.flac", Body::from(vec![0; 10])).await;
  assert_eq!(status, 201, "{answer}");
}

#[tokio::test]
async fn an_upload_is_not_held_to_the_size_of_a_json_body() {
  let app = app(TRUSTED, &[]).await;
  let (status, body) = upload(&app, "a.flac", Body::from(vec![0; 3 << 20])).await;
  assert_eq!(status, 201, "{body}");
}

#[tokio::test]
async fn an_upload_takes_the_read_write_role() {
  let keys = "[web.authentication.api_keys.viewer]\nkey = \"viewer-key-0123456789\"\n\
              role = \"readonly\"\n[web.authentication.api_keys.editor]\n\
              key = \"editor-key-0123456789\"\nrole = \"readwrite\"\n";
  let app = app(keys, &[]).await;
  let sent = |key: &str| {
    let mut sent = request("POST", "/api/v0/analysis/uploads?name=a.flac", None);
    *sent.body_mut() = Body::from(vec![0; 10]);
    with(sent, "x-api-key", key)
  };
  assert_eq!(
    app
      .send(request(
        "POST",
        "/api/v0/analysis/uploads?name=a.flac",
        None
      ))
      .await
      .status,
    401
  );
  assert_eq!(app.send(sent("viewer-key-0123456789")).await.status, 403);
  uploads_settle(&app, 0).await;
  assert_eq!(app.send(sent("editor-key-0123456789")).await.status, 201);
}

#[tokio::test]
async fn an_upload_works_like_a_file_and_stays_out_of_every_record() {
  let app = app(TRUSTED, &[]).await;
  let events = count_events(&app);
  lossless(&app, "real.flac");
  app.call("POST", &route("real.flac"), None).await;
  settled(&app).await;
  let before = events.load(Ordering::SeqCst);

  let file = std::fs::read(app.state.paths.downloads.join("real.flac")).unwrap();
  let (status, answer) = upload(&app, "My%20Song.flac", Body::from(file)).await;
  assert_eq!(status, 201, "{answer}");
  assert!(answer["path"].as_str().unwrap().ends_with("/My Song.flac"));
  let analysis = uploaded(&answer);
  assert_eq!(app.call("POST", &analysis, None).await.0, 202);
  settled(&app).await;

  // analyzed, drawn and played like any file under a root
  let (status, report) = app.call("GET", &analysis, None).await;
  assert_eq!(status, 200, "{report}");
  assert_eq!(report["root"], "temporary");
  assert_eq!(report["verdict"], "consistent");
  assert_eq!(report["transferId"], Value::Null);
  assert!(report["measures"]["peakDbfs"].as_f64().unwrap() <= 0.0);
  let spectrogram = app
    .send(request("GET", &format!("{analysis}/spectrogram"), None))
    .await;
  assert_eq!(spectrogram.status, 200);
  let content = analysis.replace("/analysis", "/content");
  assert_eq!(app.send(request("GET", &content, None)).await.status, 200);

  // and in no list, no judgment and no event
  let (_, listed) = app.call("GET", "/api/v0/analysis", None).await;
  assert_eq!(listed.as_array().unwrap().len(), 1, "only the real file");
  assert_eq!(listed[0]["root"], "downloads");
  let (status, _) = app
    .call(
      "PUT",
      &format!("{analysis}/override"),
      Some(json!({ "verdict": "fake" })),
    )
    .await;
  assert_eq!(status, 400);
  assert_eq!(events.load(Ordering::SeqCst), before);
  let listing = "/api/v0/files/temporary/directories";
  assert_eq!(app.call("GET", listing, None).await.0, 404);

  // a restart removes it with its report
  let app = app.restarted(&[]).await;
  assert_eq!(uploads_held(&app), 0);
  assert_eq!(app.call("GET", &analysis, None).await.0, 404);
  assert_eq!(app.call("POST", &analysis, None).await.0, 404);
}

#[tokio::test]
async fn an_aiff_upload_plays_through_the_wav_route() {
  let app = app(TRUSTED, &[]).await;
  let samples: Vec<u8> = (0..24).collect();
  let file = crate::files::aiff_file(2, 16, &samples);
  let (status, answer) = upload(&app, "a.aiff", Body::from(file)).await;
  assert_eq!(status, 201, "{answer}");
  let wav = uploaded(&answer).replace("/analysis", "/wav");
  let served = app.send(request("GET", &wav, None)).await;
  assert_eq!(served.status, 200);
  assert_eq!(served.headers["content-type"], "audio/wav");
}

#[tokio::test]
async fn uploads_past_their_cache_limit_go_oldest_first_with_their_reports() {
  let config = format!("{TRUSTED}{ONE_WORKER}upload_cache_max_bytes = \"1B\"\n");
  let app = app(&config, &[]).await;
  lossless(&app, "real.flac");
  let file = std::fs::read(app.state.paths.downloads.join("real.flac")).unwrap();
  let mut routes = Vec::new();
  for name in ["a.flac", "b.flac"] {
    let (status, answer) = upload(&app, name, Body::from(file.clone())).await;
    assert_eq!(status, 201, "{answer}");
    let analysis = uploaded(&answer);
    assert_eq!(app.call("POST", &analysis, None).await.0, 202);
    settled(&app).await;
    routes.push(analysis);
  }
  uploads_settle(&app, 2).await;
  let (status, report) = app.call("GET", &routes[0], None).await;
  assert_eq!(status, 200, "the two newest stay over the limit: {report}");

  let (_, answer) = upload(&app, "c.flac", Body::from(file)).await;
  uploads_settle(&app, 2).await;
  assert_eq!(app.call("GET", &routes[0], None).await.0, 404);
  assert_eq!(app.call("POST", &routes[0], None).await.0, 404);
  assert_eq!(app.call("GET", &routes[1], None).await.0, 200);
  assert_eq!(
    app.call("POST", &uploaded(&answer), None).await.0,
    202,
    "the newest is held"
  );
}

/// What the spectrograms and waveforms of real files take on disk.
fn pictures_bytes(app: &TestApp) -> u64 {
  std::fs::read_dir(app.state.paths.data.join("analysis"))
    .unwrap()
    .map(|entry| entry.unwrap().metadata().unwrap())
    .filter(std::fs::Metadata::is_file)
    .map(|metadata| metadata.len())
    .sum()
}

#[tokio::test]
async fn uploads_over_their_budget_leave_the_pictures_of_real_files() {
  let config =
    format!("remote_configuration = true\n{TRUSTED}{ONE_WORKER}upload_cache_max_bytes = \"1B\"\n");
  let app = app(&config, &[]).await;
  for name in ["a.flac", "b.flac"] {
    lossless(&app, name);
    app.call("POST", &route(name), None).await;
    settled(&app).await;
  }
  // the real files' pictures fill their budget: an upload counted against it would push one out
  let full = format!("{}B", pictures_bytes(&app));
  let full = json!({ "set": { "analysis.cache_max_bytes": full } });
  let (status, body) = app.call("PATCH", "/api/v0/options", Some(full)).await;
  assert_eq!(status, 200, "{body}");

  let file = std::fs::read(app.state.paths.downloads.join("a.flac")).unwrap();
  for name in ["x.flac", "y.flac", "z.flac"] {
    let (status, answer) = upload(&app, name, Body::from(file.clone())).await;
    assert_eq!(status, 201, "{answer}");
    assert_eq!(app.call("POST", &uploaded(&answer), None).await.0, 202);
    settled(&app).await;
  }
  uploads_settle(&app, 2).await;
  for name in ["a.flac", "b.flac"] {
    let spectrogram = format!("{}/spectrogram", route(name));
    let served = app.send(request("GET", &spectrogram, None)).await;
    assert_eq!(served.status, 200, "{name}");
  }
}
