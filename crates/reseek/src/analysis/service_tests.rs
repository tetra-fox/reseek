//! The analysis calls against the in-memory daemon: a downloaded file analysed by its path and
//! by its download, its report and its spectrogram, the event and the frame raised once, files
//! outside the roots refused, unreadable files reported rather than failed, and the user's
//! judgement kept across a restart.

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
async fn a_download_is_analysed_reported_drawn_and_announced_once() {
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
  assert_eq!(status, 409, "queued, running or analysed already");
  let frames = stream.until("files.analysis").await;
  let frame = &frames.last().unwrap().data;
  assert_eq!(frame["transferId"], json!(id));
  assert_eq!(frame["verdict"], "consistent");
  settled(&app).await;

  let (status, report) = send(&app, "GET", &route("Album/01.flac"), None).await;
  assert_eq!(status, 200, "{report}");
  assert_eq!(report["verdict"], "consistent");
  assert_eq!(report["path"], local.to_string_lossy().as_ref());
  assert_eq!(report["claimed"]["codec"], "flac");
  assert_eq!(report["claimed"]["bitDepth"], 16);
  assert!(report["windows"].as_array().unwrap().len() >= 30);
  assert_eq!(report["override"], Value::Null);
  let url = report["spectrogramUrl"].as_str().unwrap();
  assert_eq!(url, format!("{}/spectrogram.png", route("Album/01.flac")));
  let by_download = format!("/api/v0/transfers/downloads/alice/{id}/analysis?windows=false");
  let (status, same) = send(&app, "GET", &by_download, None).await;
  assert_eq!(status, 200);
  assert_eq!(same["windows"], Value::Null);
  assert_eq!(same["analysedAt"], report["analysedAt"]);

  let guided = app.send(request("GET", url, None)).await;
  assert_eq!(guided.status, 200);
  assert_eq!(guided.headers["content-type"], "image/png");
  assert!(guided.body.starts_with(b"\x89PNG"));
  let plain = app
    .send(request("GET", &format!("{url}?guides=false"), None))
    .await;
  assert!(plain.body.starts_with(b"\x89PNG"));
  assert_ne!(guided.body, plain.body);

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
  let image = format!("{}/spectrogram.png", route("noise.flac"));
  let (status, _) = send(&app, "GET", &image, None).await;
  assert_eq!(status, 404);
}

#[tokio::test]
async fn the_judgement_is_kept_across_a_restart_and_beside_a_new_analysis() {
  let app = app(CONFIG, &[]).await;
  lossless(&app, "a.flac");
  let judgement = format!("{}/override", route("a.flac"));
  let fake = json!({ "verdict": "fake", "note": "sounds like 128k" });
  let (status, _) = send(&app, "PUT", &judgement, Some(fake.clone())).await;
  assert_eq!(status, 404, "nothing to judge before an analysis");
  send(&app, "POST", &route("a.flac"), None).await;
  settled(&app).await;
  let (status, _) = send(&app, "PUT", &judgement, Some(fake)).await;
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

  let (status, _) = send(&app, "DELETE", &judgement, None).await;
  assert_eq!(status, 204);
  let (_, report) = send(&app, "GET", &route("a.flac"), None).await;
  assert_eq!(report["override"], Value::Null);
}

#[tokio::test]
async fn spectrograms_beyond_the_cache_limit_go_least_recently_viewed_first() {
  let app = app(
    &format!("{CONFIG}[analysis]\ncache_max_bytes = \"1B\"\n"),
    &[],
  )
  .await;
  lossless(&app, "a.flac");
  lossless(&app, "b.flac");
  send(&app, "POST", &route("a.flac"), None).await;
  settled(&app).await;
  send(&app, "POST", &route("b.flac"), None).await;
  settled(&app).await;
  let a = format!("{}/spectrogram.png", route("a.flac"));
  let b = format!("{}/spectrogram.png", route("b.flac"));
  let (status, gone) = send(&app, "GET", &a, None).await;
  assert_eq!(status, 404, "{gone}");
  assert_eq!(app.send(request("GET", &b, None)).await.status, 200);
  let images = app.state.paths.data.join("analysis");
  let kept: Vec<_> = std::fs::read_dir(&images).unwrap().collect();
  assert_eq!(kept.len(), 2, "the newest report's two images");
}
