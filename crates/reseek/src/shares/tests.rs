//! The engine as a whole: the API, scanning at startup and loading the index again, the scan
//! pending after an option change, and watched changes.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use soulseek_proto::SearchQuery;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::testing::{TestApp, app, request};

/// A configuration sharing `dir/music` as `Music`, excluding `dir/music/private`, with the
/// shares in memory and loopback an administrator.
fn config(dir: &Path) -> String {
  let music = dir.join("music");
  std::fs::create_dir_all(music.join("Album")).unwrap();
  std::fs::create_dir_all(music.join("private")).unwrap();
  std::fs::write(music.join("Album/01.mp3"), "one").unwrap();
  std::fs::write(music.join("private/p.mp3"), "p").unwrap();
  format!(
    "[shares]\ndirectories = [\"[Music]{}\", \"!{}\"]\n\
     [web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n",
    music.display(),
    music.join("private").display()
  )
}

async fn shared_app() -> (tempfile::TempDir, TestApp) {
  let files = tempfile::tempdir().unwrap();
  let app = app(&config(files.path()), &[]).await;
  (files, app)
}

/// Waits until `check` holds, for at most ten seconds.
async fn eventually(mut check: impl AsyncFnMut() -> bool) {
  for _ in 0..200 {
    if check().await {
      return;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
  panic!("never held");
}

async fn names(shares: &Shares, term: &str) -> Vec<String> {
  let query = SearchQuery {
    terms: vec![term.to_string()],
    exclusions: Vec::new(),
  };
  shares
    .search(&query, None)
    .await
    .into_iter()
    .map(|f| f.filename.as_str().to_string())
    .collect()
}

#[tokio::test]
async fn the_startup_scan_fills_the_index_and_the_api_shows_it() {
  let (_files, app) = shared_app().await;
  let state = app.state.state.current();
  assert!(state.shares.ready && !state.shares.scanning && !state.shares.scan_pending);
  assert_eq!(
    state.shares.hosts.as_deref(),
    Some(&["local".to_string()][..])
  );
  assert_eq!((state.shares.directories, state.shares.files), (2, 1));
  let listed = app.send(request("GET", "/api/v0/shares", None)).await;
  assert_eq!(listed.status, 200);
  let local = listed.json()["local"].clone();
  assert_eq!(local[0]["alias"], "private");
  assert_eq!(local[0]["isExcluded"], true);
  assert_eq!(local[0]["directories"], serde_json::Value::Null);
  assert_eq!(local[1]["alias"], "Music");
  assert_eq!(
    local[1]["directories"], 2,
    "the share's own directory counts"
  );
  let id = local[1]["id"].as_str().unwrap().to_string();
  let contents = app
    .send(request(
      "GET",
      &format!("/api/v0/shares/{id}/contents"),
      None,
    ))
    .await
    .json();
  assert_eq!(contents[0]["name"], "Music");
  assert_eq!(contents[1]["name"], "Music\\Album");
  assert_eq!(contents[1]["files"][0]["filename"], "01.mp3");
  let unknown = app
    .send(request("GET", "/api/v0/shares/ABC/contents", None))
    .await;
  assert_eq!(unknown.status, 404);
  assert_eq!(
    names(&app.state.shares, "album").await,
    ["Music\\Album\\01.mp3"]
  );
  let found = app
    .state
    .shares
    .resolve_file("Music\\Album\\01.mp3")
    .await
    .unwrap();
  assert_eq!(found.unwrap().size, 3);
  assert_eq!(
    *app.state.shares.shared_counts().borrow(),
    SharedCounts {
      directories: 2,
      files: 1
    }
  );
  assert!(app.state.shares.browse_cache().exists());
}

#[tokio::test]
async fn a_scan_is_one_at_a_time_and_only_a_running_one_cancels() {
  let (_files, app) = shared_app().await;
  let shares = &app.state.shares;
  let held = CancellationToken::new();
  *lock(&shares.running) = Some(held.clone());
  let scan = app.send(request("PUT", "/api/v0/shares", None)).await;
  assert_eq!(scan.status, 409);
  assert_eq!(scan.json()["error"], "A share scan is already in progress.");
  let cancel = app.send(request("DELETE", "/api/v0/shares", None)).await;
  assert_eq!(cancel.status, 204);
  assert!(held.is_cancelled());
  *lock(&shares.running) = None;
  let none = app.send(request("DELETE", "/api/v0/shares", None)).await;
  assert_eq!(none.status, 409);
  let forced = app
    .send(request("PUT", "/api/v0/shares?force=true", None))
    .await;
  assert_eq!(forced.status, 204);
  eventually(async || lock(&shares.running).is_none()).await;
  assert!(app.state.state.current().shares.ready);
}

#[tokio::test]
async fn a_changed_share_list_marks_a_scan_pending() {
  let (files, app) = shared_app().await;
  let mut changed = config(files.path());
  changed = changed.replace("[shares]\n", "[shares]\nfilters = [\"\\\\.mp3$\"]\n");
  std::fs::write(app.dir.path().join("config.toml"), changed).unwrap();
  app.state.config.reload().await.unwrap();
  eventually(async || app.state.state.current().shares.scan_pending).await;
}

#[tokio::test]
async fn a_restart_loads_the_backup_and_scans_once_reusing_it() {
  let (files, app) = shared_app().await;
  let paths = app.state.paths.clone();
  assert!(storage::backup_path(&paths.cache).exists());
  let again = Shares::open(
    app.state.config.clone(),
    &paths,
    app.state.state.clone(),
    CancellationToken::new(),
  )
  .await
  .unwrap();
  again.initialized().await;
  assert!(
    lock(&again.running).is_some(),
    "the scan after loading runs in the background"
  );
  eventually(async || lock(&again.running).is_none()).await;
  let scans = again
    .db
    .read(|conn| repository::scans_since(conn, 0))
    .await
    .unwrap();
  assert_eq!(scans.len(), 2, "the loaded scan and the one after loading");
  assert_eq!(names(&again, "album").await, ["Music\\Album\\01.mp3"]);
  drop(files);
}

#[tokio::test]
async fn watched_changes_are_applied_by_what_is_on_disk() {
  let (files, app) = shared_app().await;
  let shutdown = CancellationToken::new();
  let mut paths = app.state.paths.clone();
  paths.cache = files.path().join("cache");
  std::fs::create_dir_all(&paths.cache).unwrap();
  let shares = Shares::open_with(
    app.state.config.clone(),
    &paths,
    app.state.state.clone(),
    shutdown.clone(),
    Duration::from_millis(100),
  )
  .await
  .unwrap();
  shares.initialized().await;
  let shares: Arc<Shares> = shares;
  // the watches are in place again once the scan after loading is done
  eventually(async || lock(&shares.running).is_none() && lock(&shares.watches).is_some()).await;
  let album = files.path().join("music/Album");
  std::fs::write(album.join("02.mp3"), "two").unwrap();
  eventually(async || names(&shares, "02").await == ["Music\\Album\\02.mp3"]).await;
  std::fs::rename(album.join("02.mp3"), album.join("03.mp3")).unwrap();
  eventually(async || {
    names(&shares, "02").await.is_empty() && names(&shares, "03").await.len() == 1
  })
  .await;
  let disc = files.path().join("music/Disc 2");
  std::fs::create_dir_all(disc.join("Inner")).unwrap();
  std::fs::write(disc.join("Inner/04.mp3"), "four").unwrap();
  eventually(async || names(&shares, "04").await == ["Music\\Disc 2\\Inner\\04.mp3"]).await;
  std::fs::write(files.path().join("music/private/q.mp3"), "q").unwrap();
  std::fs::remove_dir_all(&disc).unwrap();
  eventually(async || names(&shares, "04").await.is_empty()).await;
  assert!(
    names(&shares, "q").await.is_empty(),
    "an excluded directory stays out"
  );
  eventually(async || lock(&shares.status).files == 2).await;
  shutdown.cancel();
}

#[tokio::test]
async fn a_startup_scan_leaves_a_held_slot_to_its_scan() {
  let (files, app) = shared_app().await;
  let shares = app.state.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  let held = shares.take_slot().unwrap();
  assert!(shares.scan_at_startup(false).await);
  assert!(
    lock(&shares.running)
      .as_ref()
      .is_some_and(|cancel| !cancel.is_cancelled()),
    "the slot is still the first scan's"
  );
  assert!(shares.take_slot().is_none());
  drop(held);
  *lock(&shares.running) = None;
  drop(files);
}

#[tokio::test]
async fn lost_events_rescan_the_directory_and_drop_what_is_gone() {
  let (files, app) = shared_app().await;
  let shares = app.state.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  let album = files.path().join("music/Album");
  std::fs::remove_file(album.join("01.mp3")).unwrap();
  std::fs::write(album.join("02.mp3"), "two").unwrap();
  let settings = Arc::new(scanner::Settings::new(
    shares.listed(),
    &app.state.config.current().values,
  ));
  let roots = vec![files.path().join("music")];
  watcher::apply(
    &shares,
    &settings,
    &roots,
    vec![(album, watcher::Need::Rescan)],
  )
  .await
  .unwrap();
  assert!(
    names(&shares, "01").await.is_empty(),
    "what is gone is pruned"
  );
  assert_eq!(names(&shares, "02").await, ["Music\\Album\\02.mp3"]);
  drop(files);
}

#[tokio::test]
async fn a_faulted_scan_keeps_the_served_index_ready_and_marks_a_scan_pending() {
  let (files, app) = shared_app().await;
  let shares = app.state.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  let before = lock(&shares.status).clone();
  assert!(before.ready);
  shares.publish(|status| {
    status.scanning = true;
    status.ready = false;
  });
  shares.faulted("the index refused a write", &before);
  let shown = app.state.state.current().shares.clone();
  assert!(shown.ready, "the index from before is still served");
  assert!(shown.faulted && shown.scan_pending && !shown.scanning);
  assert_eq!(shown.files, before.files);
  drop(files);
}

#[tokio::test]
async fn the_retention_check_rescans_only_without_a_recent_trusted_scan() {
  let files = tempfile::tempdir().unwrap();
  let config = config(files.path()).replace(
    "[shares]\n",
    "[shares.cache]\nretention = \"1h\"\n[shares]\n",
  );
  let app = app(&config, &[]).await;
  let shares = app.state.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  assert!(!shares.retention_tick().await, "the startup scan is recent");
  shares
    .db
    .write(|conn| repository::mark_latest_suspect(conn))
    .await
    .unwrap();
  assert!(
    shares.retention_tick().await,
    "a suspect scan is no trusted one"
  );
  eventually(async || lock(&shares.running).is_none()).await;
}
