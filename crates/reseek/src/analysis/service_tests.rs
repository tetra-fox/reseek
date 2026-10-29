//! The analysis calls against the in-memory daemon: a downloaded file analyzed by its path and
//! by its download, its report, its spectrogram and its waveform, the event and the frame raised
//! once, files outside the roots refused, unreadable files reported rather than failed, and the
//! user's judgment kept across a restart.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use api_types::events::EventType;
use api_types::transfers::TransferStates;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use super::fixtures::{music, pink, write_flac};
use crate::testing::{TestApp, app, request};
use crate::transfers::{Record, store as transfers};

const CONFIG: &str = "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n";

async fn send(app: &TestApp, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
  let answer = app.send(request(method, uri, body)).await;
  let json = serde_json::from_slice(&answer.body).unwrap_or(Value::Null);
  (answer.status.as_u16(), json)
}

/// The analysis route of the file `relative` under the downloads directory.
fn route(relative: &str) -> String {
  format!(
    "/api/v0/files/downloads/files/{}/analysis",
    URL_SAFE_NO_PAD.encode(relative)
  )
}

/// Writes a lossless FLAC of generated music at `relative` under the downloads directory.
fn lossless(app: &TestApp, relative: &str) {
  let path = app.state.paths.downloads.join(relative);
  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  write_flac(&music(1, 44_100, pink), 16, false, &path);
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
    let (status, jobs) = send(app, "GET", "/api/v0/analysis/jobs", None).await;
    assert_eq!(status, 200);
    if jobs.as_array().unwrap().is_empty() {
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
  let app = app(CONFIG, &[]).await;
  lossless(&app, "Album/01.flac");
  let mut record = Record::download("alice", "@@share\\Album\\01.flac", 3, None);
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

  let (status, created) = send(&app, "POST", &route("Album/01.flac"), None).await;
  assert_eq!(status, 202, "{created}");
  assert!(created["jobId"].is_string());
  let (status, _) = send(&app, "POST", &route("Album/01.flac"), None).await;
  assert_eq!(status, 409, "queued, running or analyzed already");
  let frames = stream.until("files.analysis").await;
  let frame = &frames.last().unwrap().data;
  assert_eq!(frame["transferId"], json!(id));
  assert_eq!(frame["verdict"], "consistent");
  settled(&app).await;

  let (status, report) = send(&app, "GET", &route("Album/01.flac"), None).await;
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
  let (status, same) = send(&app, "GET", &by_download, None).await;
  assert_eq!(status, 200);
  assert_eq!(same["windows"], Value::Null);
  assert_eq!(same["analyzedAt"], report["analyzedAt"]);
  let (_, listed) = send(&app, "GET", "/api/v0/analysis", None).await;
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
  let (status, _) = send(&app, "POST", &route("Album/01.flac"), None).await;
  assert_eq!(status, 409, "the report is current");
  let (status, _) = send(
    &app,
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
  let app = app(CONFIG, &[]).await;
  let elsewhere = tempfile::tempdir().unwrap();
  let outside = elsewhere.path().join("x.flac");
  write_flac(&music(1, 44_100, pink), 16, false, &outside);
  let downloads = &app.state.paths.downloads;
  std::fs::create_dir_all(downloads).unwrap();
  std::os::unix::fs::symlink(&outside, downloads.join("link.flac")).unwrap();
  let (status, body) = send(&app, "POST", &route("link.flac"), None).await;
  assert_eq!(status, 403, "{body}");
  let (status, _) = send(&app, "GET", &route("link.flac"), None).await;
  assert_eq!(status, 403);
  let escape = format!(
    "/api/v0/files/downloads/files/{}/analysis",
    URL_SAFE_NO_PAD.encode("../x.flac")
  );
  let (status, _) = send(&app, "POST", &escape, None).await;
  assert_eq!(status, 400);
  let (status, _) = send(&app, "POST", &route("missing.flac"), None).await;
  assert_eq!(status, 404);
}

#[tokio::test]
async fn unreadable_files_are_reported_inconclusive_not_failed() {
  let app = app(CONFIG, &[]).await;
  let downloads = app.state.paths.downloads.clone();
  std::fs::create_dir_all(&downloads).unwrap();
  lossless(&app, "whole.flac");
  let whole = std::fs::read(downloads.join("whole.flac")).unwrap();
  std::fs::write(downloads.join("cut.flac"), &whole[..whole.len() / 50]).unwrap();
  std::fs::write(downloads.join("noise.flac"), b"this is not audio at all").unwrap();
  std::fs::write(downloads.join("empty.wav"), b"").unwrap();
  for name in ["cut.flac", "noise.flac", "empty.wav"] {
    let (status, _) = send(&app, "POST", &route(name), None).await;
    assert_eq!(status, 202, "{name}");
  }
  settled(&app).await;
  for name in ["cut.flac", "noise.flac", "empty.wav"] {
    let (status, report) = send(&app, "GET", &route(name), None).await;
    assert_eq!(status, 200, "{name}: {report}");
    assert_eq!(report["verdict"], "inconclusive", "{name}");
    assert!(
      !report["limitations"].as_array().unwrap().is_empty(),
      "{name}: {report}"
    );
  }
  let (_, noise) = send(&app, "GET", &route("noise.flac"), None).await;
  assert_eq!(noise["claimed"]["codec"], "unknown");
  assert_eq!(noise["spectrogram"], Value::Null);
  let pictures = format!("{}/spectrogram", route("noise.flac"));
  let (status, _) = send(&app, "GET", &pictures, None).await;
  assert_eq!(status, 404);
}

#[tokio::test]
async fn the_judgment_is_kept_across_a_restart_and_beside_a_new_analysis() {
  let app = app(CONFIG, &[]).await;
  lossless(&app, "a.flac");
  let judgment = format!("{}/override", route("a.flac"));
  let fake = json!({ "verdict": "fake", "note": "sounds like 128k" });
  let (status, _) = send(&app, "PUT", &judgment, Some(fake.clone())).await;
  assert_eq!(status, 404, "nothing to judge before an analysis");
  send(&app, "POST", &route("a.flac"), None).await;
  settled(&app).await;
  let (status, _) = send(&app, "PUT", &judgment, Some(fake)).await;
  assert_eq!(status, 204);

  let app = app.restarted(&[]).await;
  let (_, report) = send(&app, "GET", &route("a.flac"), None).await;
  assert_eq!(
    report["verdict"], "consistent",
    "the reading stays beside it"
  );
  assert_eq!(report["override"]["verdict"], "fake");
  assert_eq!(report["override"]["note"], "sounds like 128k");
  assert!(report["override"]["by"].is_string());
  let (_, listed) = send(&app, "GET", "/api/v0/analysis", None).await;
  assert_eq!(listed[0]["override"]["verdict"], "fake");
  assert_eq!(listed[0]["windows"], Value::Null);

  send(
    &app,
    "POST",
    &format!("{}?force=true", route("a.flac")),
    None,
  )
  .await;
  settled(&app).await;
  let (_, report) = send(&app, "GET", &route("a.flac"), None).await;
  assert_eq!(
    report["override"]["verdict"], "fake",
    "a new analysis keeps it"
  );

  let (status, _) = send(&app, "DELETE", &judgment, None).await;
  assert_eq!(status, 204);
  let (_, report) = send(&app, "GET", &route("a.flac"), None).await;
  assert_eq!(report["override"], Value::Null);
}

#[tokio::test]
async fn a_report_and_its_judgment_outlive_the_file() {
  let app = app(CONFIG, &[]).await;
  lossless(&app, "Album/02.flac");
  let mut record = Record::download("alice", "@@share\\Album\\02.flac", 3, None);
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
  send(&app, "POST", &by_download, None).await;
  settled(&app).await;
  let fake = json!({ "verdict": "fake", "note": "a transcode" });
  let (status, _) = send(&app, "PUT", &format!("{by_download}/override"), Some(fake)).await;
  assert_eq!(status, 204);

  std::fs::remove_file(&local).unwrap();
  let (status, report) = send(&app, "GET", &by_download, None).await;
  assert_eq!(status, 200, "{report}");
  assert_eq!(report["override"]["verdict"], "fake");
  let limitations = report["limitations"].as_array().unwrap();
  assert!(
    limitations.contains(&json!("The file is no longer there.")),
    "{report}"
  );
  let (status, _) = send(&app, "GET", &route("Album/02.flac"), None).await;
  assert_eq!(status, 200, "by its path too");
  let image = app
    .send(request("GET", &format!("{by_download}/spectrogram"), None))
    .await;
  assert_eq!(image.status, 200);
  let legit = json!({ "verdict": "legit" });
  let (status, _) = send(&app, "PUT", &format!("{by_download}/override"), Some(legit)).await;
  assert_eq!(status, 204);
  let (status, _) = send(&app, "DELETE", &format!("{by_download}/override"), None).await;
  assert_eq!(status, 204);
  let (status, _) = send(&app, "POST", &format!("{by_download}?force=true"), None).await;
  assert_eq!(status, 404, "a file that is gone can not be analyzed again");
}

#[tokio::test]
async fn spectrograms_beyond_the_cache_limit_go_least_recently_viewed_first() {
  let app = app(&format!("remote_configuration = true\n{CONFIG}"), &[]).await;
  for name in ["a.flac", "b.flac", "c.flac"] {
    lossless(&app, name);
  }
  for name in ["a.flac", "b.flac"] {
    send(&app, "POST", &route(name), None).await;
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
  let (status, body) = send(&app, "PATCH", "/api/v0/options", Some(lowered)).await;
  assert_eq!(status, 200, "{body}");
  for _ in 0..200 {
    if kept(&app) == 1 {
      break;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
  assert_eq!(kept(&app), 1, "the most recently viewed kept");
  let (status, gone) = send(&app, "GET", &spectrogram("b.flac"), None).await;
  assert_eq!(status, 404, "{gone}");

  // a new analysis over the limit removes the least recently viewed
  send(&app, "POST", &route("c.flac"), None).await;
  settled(&app).await;
  assert_eq!(kept(&app), 1, "the newest report's file");
  assert_eq!(send(&app, "GET", &spectrogram("a.flac"), None).await.0, 404);
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
  let app = app(CONFIG, &[]).await;
  for name in ["a.flac", "b.flac", "c.flac"] {
    lossless(&app, name);
  }
  let mut ids = Vec::new();
  for name in ["a.flac", "b.flac", "c.flac"] {
    let (status, created) = send(&app, "POST", &route(name), None).await;
    assert_eq!(status, 202);
    ids.push(created["jobId"].as_str().unwrap().to_string());
  }
  // one worker: the last is still waiting
  let (status, _) = send(
    &app,
    "DELETE",
    &format!("/api/v0/analysis/jobs/{}", ids[2]),
    None,
  )
  .await;
  assert_eq!(status, 204);
  let (_, jobs) = send(&app, "GET", "/api/v0/analysis/jobs", None).await;
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
  assert_eq!(send(&app, "DELETE", &unknown, None).await.0, 404);
  settled(&app).await;
  let (status, _) = send(&app, "GET", &route("c.flac"), None).await;
  assert_eq!(status, 404, "the canceled file was not analyzed");

  let (status, window) = send(&app, "GET", &format!("{}/windows/3", route("a.flac")), None).await;
  assert_eq!(status, 200, "{window}");
  assert_eq!(window["t"], 3.0);
  assert_eq!(window["hz"].as_array().unwrap().len(), 512);
  assert_eq!(window["db"].as_array().unwrap().len(), 512);
  // linear bins, lowest first, to the top of the band
  let hz = |i: usize| window["hz"][i].as_f64().unwrap();
  assert!((hz(0) - 21.5).abs() < 0.1, "{}", hz(0));
  assert!((hz(511) - 22_028.5).abs() < 0.1, "{}", hz(511));
  let (status, _) = send(
    &app,
    "GET",
    &format!("{}/windows/999", route("a.flac")),
    None,
  )
  .await;
  assert_eq!(status, 404);
}

#[tokio::test]
async fn a_report_from_an_older_version_lists_as_stale_keeps_its_judgment_and_is_replaced() {
  let app = app(CONFIG, &[]).await;
  lossless(&app, "a.flac");
  send(&app, "POST", &route("a.flac"), None).await;
  settled(&app).await;
  let judgment = json!({ "verdict": "fake", "note": "sounds like 128k" });
  let judged = format!("{}/override", route("a.flac"));
  assert_eq!(send(&app, "PUT", &judged, Some(judgment)).await.0, 204);
  // the report as an older version stored it, numbered as an older shape
  let stored = rusqlite::Connection::open(app.state.paths.data.join("analysis.db")).unwrap();
  stored
    .execute("UPDATE reports SET format = format - 1", [])
    .unwrap();

  let (status, listed) = send(&app, "GET", "/api/v0/analysis?offset=0&limit=500", None).await;
  assert_eq!(status, 200, "{listed}");
  let entry = &listed[0];
  assert_eq!(entry["stale"], true);
  assert_eq!(entry["relativePath"], "a.flac");
  assert_eq!(entry["override"]["note"], "sounds like 128k");
  let (status, report) = send(&app, "GET", &route("a.flac"), None).await;
  assert_eq!(status, 200, "{report}");
  assert_eq!(report["stale"], true);
  assert_eq!(report["override"]["verdict"], "fake");
  assert_eq!(report["spectrogram"], Value::Null);
  let pictures = format!("{}/spectrogram", route("a.flac"));
  assert_eq!(app.send(request("GET", &pictures, None)).await.status, 404);

  // analyzing again needs no force: a stale report is no reading of the file
  let (status, created) = send(&app, "POST", &route("a.flac"), None).await;
  assert_eq!(status, 202, "{created}");
  settled(&app).await;
  let (_, report) = send(&app, "GET", &route("a.flac"), None).await;
  assert_eq!(report["stale"], false);
  assert!(report["spectrogram"]["durationSeconds"].is_number());
  assert_eq!(
    report["override"]["note"], "sounds like 128k",
    "the judgment stays"
  );
}
