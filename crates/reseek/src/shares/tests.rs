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
  assert_eq!(scan.json()["error"], "A share scan is already in progress");
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
async fn changed_filters_mark_a_scan_pending() {
  let (files, app) = shared_app().await;
  let mut changed = config(files.path());
  changed = changed.replace("[shares]\n", "[shares]\nfilters = [\"\\\\.mp3$\"]\n");
  std::fs::write(app.dir.path().join("config.toml"), changed).unwrap();
  app.state.config.reload().await.unwrap();
  eventually(async || app.state.state.current().shares.scan_pending).await;
}

#[tokio::test]
async fn filters_only_reordered_or_repeated_mark_no_scan_pending() {
  let (_files, app) = shared_app().await;
  let shares = &app.state.shares;
  eventually(async || lock(&shares.running).is_none()).await;
  let mut options = app.state.config.current().values.clone();
  options.shares.filters = vec!["a$".into(), "b$".into()];
  shares.apply_options(&options);
  // a scan would clear what the first change marked
  shares.publish(|status| status.scan_pending = false);
  options.shares.filters = vec!["b$".into(), "a$".into(), "b$".into()];
  shares.apply_options(&options);
  assert!(!app.state.state.current().shares.scan_pending);
  options.shares.filters = vec!["b$".into()];
  shares.apply_options(&options);
  assert!(app.state.state.current().shares.scan_pending);
}

#[tokio::test]
async fn a_removed_share_marks_a_scan_pending_and_is_no_longer_watched() {
  let files = tempfile::tempdir().unwrap();
  let other = files.path().join("other");
  std::fs::create_dir_all(&other).unwrap();
  let base = config(files.path());
  let both = base.replace(
    "directories = [",
    &format!("directories = [\"[Other]{}\", ", other.display()),
  );
  let app = app(&both, &[]).await;
  let shares = app.state.shares.clone();
  eventually(async || lock(&shares.running).is_none() && lock(&shares.watches).is_some()).await;
  let watched = |shares: &Shares| {
    lock(&shares.watches)
      .as_ref()
      .map(|w| w.roots.clone())
      .unwrap_or_default()
  };
  assert!(watched(&shares).contains(&other));
  std::fs::write(app.dir.path().join("config.toml"), base).unwrap();
  app.state.config.reload().await.unwrap();
  eventually(async || app.state.state.current().shares.scan_pending).await;
  assert!(
    !watched(&shares).contains(&other),
    "a removed share's watch is dropped at once"
  );
  assert_eq!(shares.listed().iter().filter(|s| !s.excluded).count(), 1);
}

/// What the index holds as the attributes of `Music\Album\01.mp3`.
async fn attributes(shares: &Shares) -> String {
  shares
    .db
    .read(|conn| {
      conn.query_row(
        "SELECT attributeJson FROM files WHERE maskedFilename = 'Music\\Album\\01.mp3'",
        [],
        |row| row.get::<_, String>(0),
      )
    })
    .await
    .unwrap()
}

/// Attributes no read of `01.mp3` gives, so a row that still has them was reused.
const MARKER: &str = r#"[{"type":"BitRate","value":12345}]"#;

/// Puts [`MARKER`] in place of the attributes of `Music\Album\01.mp3`.
async fn mark_attributes(shares: &Shares) {
  shares
    .db
    .write(|conn| {
      conn.execute(
        "UPDATE files SET attributeJson = ?1 WHERE maskedFilename = 'Music\\Album\\01.mp3'",
        [MARKER],
      )
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn an_index_that_fails_to_load_is_scanned_again_without_reusing_rows() {
  let (files, app) = shared_app().await;
  let shares = app.state.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  mark_attributes(&shares).await;
  shares
    .db
    .write(|conn| repository::mark_latest_suspect(conn))
    .await
    .unwrap();
  let paths = app.state.paths.clone();
  storage::back_up(&shares.db, &storage::backup_path(&paths.cache))
    .await
    .unwrap();
  let again = Shares::open(
    app.state.config.clone(),
    &paths,
    app.state.state.clone(),
    CancellationToken::new(),
  )
  .await
  .unwrap();
  again.initialized().await;
  assert_ne!(attributes(&again).await, MARKER);
  drop(files);
}

#[tokio::test]
async fn a_restart_loads_the_backup_and_scans_once() {
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
async fn a_scan_holding_the_slot_at_startup_is_not_overwritten_by_a_load() {
  let (files, app) = shared_app().await;
  let paths = app.state.paths.clone();
  let again = Shares::open(
    app.state.config.clone(),
    &paths,
    app.state.state.clone(),
    CancellationToken::new(),
  )
  .await
  .unwrap();
  // a scan asked for over the API before the startup load ran
  let held = again.take_slot().unwrap();
  again.initialized().await;
  let status = lock(&again.status).clone();
  assert!(
    !status.ready && !status.loaded,
    "the index is left to the scan that holds the slot"
  );
  drop(held);
  *lock(&again.running) = None;
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
  let settings = Arc::new(shares.scan_settings());
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

#[tokio::test]
async fn changes_waiting_for_quiet_are_applied_when_the_watches_are_replaced() {
  let (files, app) = shared_app().await;
  let mut paths = app.state.paths.clone();
  paths.cache = files.path().join("cache");
  std::fs::create_dir_all(&paths.cache).unwrap();
  let shares = Shares::open_with(
    app.state.config.clone(),
    &paths,
    app.state.state.clone(),
    CancellationToken::new(),
    Duration::from_secs(600),
  )
  .await
  .unwrap();
  shares.initialized().await;
  eventually(async || lock(&shares.running).is_none() && lock(&shares.watches).is_some()).await;
  std::fs::write(files.path().join("music/Album/05.mp3"), "five").unwrap();
  // the notification needs a moment to reach the watcher before the watches are replaced
  tokio::time::sleep(Duration::from_millis(300)).await;
  let settings = Arc::new(shares.scan_settings());
  shares.watch_shares(settings).await;
  eventually(async || names(&shares, "05").await == ["Music\\Album\\05.mp3"]).await;
  drop(files);
}

#[tokio::test]
async fn a_canceled_scan_leaves_the_engine_idle_at_full_progress() {
  let (files, app) = shared_app().await;
  let shares = app.state.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  let cancel = shares.take_slot().unwrap();
  cancel.cancel();
  assert!(!shares.run_scan(false, cancel).await);
  let shown = app.state.state.current().shares.clone();
  assert!(shown.cancelled && shown.scan_pending && !shown.scanning);
  assert_eq!(shown.scan_progress, 1.0);
  assert!(shown.ready, "the index from before is still served");
  drop(files);
}

/// Adds `dir/other`, holding one file, to the shared directories as `Other`.
fn add_other(app: &TestApp, files: &Path) {
  let other = files.join("other");
  std::fs::create_dir_all(&other).unwrap();
  std::fs::write(other.join("x.mp3"), "x").unwrap();
  let mut options = app.state.config.current().values.clone();
  options
    .shares
    .directories
    .push(format!("[Other]{}", other.display()));
  app.state.shares.apply_options(&options);
}

/// The counts of the configured share `alias`.
fn counts(shares: &Shares, alias: &str) -> (Option<u32>, Option<u32>) {
  let listed = shares.listed();
  let share = listed.iter().find(|s| s.alias == alias).unwrap();
  (share.directories, share.files)
}

#[tokio::test]
async fn a_share_added_since_the_last_scan_is_not_counted_by_watched_changes() {
  let (files, app) = shared_app().await;
  let shares = app.state.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  add_other(&app, files.path());
  let album = files.path().join("music/Album");
  std::fs::write(album.join("02.mp3"), "two").unwrap();
  watcher::apply(
    &shares,
    &shares.filled_from(),
    &[files.path().join("music")],
    vec![(album.join("02.mp3"), watcher::Need::Change)],
  )
  .await
  .unwrap();
  assert_eq!(counts(&shares, "Music"), (Some(2), Some(2)));
  assert_eq!(
    counts(&shares, "Other"),
    (None, None),
    "no scan has covered it"
  );
}

#[tokio::test]
async fn options_changed_while_a_scan_runs_stay_pending_and_uncounted() {
  let (files, app) = shared_app().await;
  let shares = app.state.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  // the scan has made its settings and waits for the writer while the options change
  let (release, held) = std::sync::mpsc::channel::<()>();
  let (taken, writer_taken) = tokio::sync::oneshot::channel();
  let hold = {
    let db = shares.db.clone();
    tokio::spawn(async move {
      db.write(move |_| {
        taken.send(()).unwrap();
        held.recv().unwrap();
        Ok(())
      })
      .await
    })
  };
  writer_taken.await.unwrap();
  shares.scan(false).unwrap();
  eventually(async || app.state.state.current().shares.scanning).await;
  add_other(&app, files.path());
  release.send(()).unwrap();
  hold.await.unwrap().unwrap();
  eventually(async || lock(&shares.running).is_none()).await;
  let shown = app.state.state.current().shares.clone();
  assert!(shown.ready && !shown.scanning);
  assert!(shown.scan_pending, "the scan did not cover the share added");
  assert_eq!(counts(&shares, "Other"), (None, None));
  assert_eq!(counts(&shares, "Music"), (Some(2), Some(1)));
  shares.scan(false).unwrap();
  eventually(async || lock(&shares.running).is_none()).await;
  assert!(!app.state.state.current().shares.scan_pending);
  assert_eq!(counts(&shares, "Other"), (Some(1), Some(1)));
}

#[tokio::test]
async fn an_agents_index_is_served_beside_the_local_one_until_it_goes() {
  serves_an_agent("").await;
}

#[tokio::test]
async fn an_agents_index_on_disk_is_served_the_same() {
  serves_an_agent("[shares.cache]\nstorage_mode = \"disk\"\n").await;
}

/// A controller whose configuration ends in `extra` loads, serves and drops an agent's index.
async fn serves_an_agent(extra: &str) {
  let files = tempfile::tempdir().unwrap();
  let app = app(&(config(files.path()) + extra), &[]).await;
  // the agent's index is another engine's dump, with a share of its own
  let (agent_files, agent) = shared_app().await;
  std::fs::create_dir_all(agent_files.path().join("music/Other")).unwrap();
  std::fs::write(agent_files.path().join("music/Other/02.flac"), "agent").unwrap();
  agent.state.shares.scan(false).unwrap();
  eventually(async || agent.state.state.current().shares.files == 2).await;
  let dump = agent_files.path().join("dump.db");
  agent.state.shares.dump(&dump).await.unwrap();
  let listed = agent.state.shares.local_shares();

  let shares = &app.state.shares;
  let counted = shares
    .load_host("nas", listed.clone(), &dump)
    .await
    .unwrap();
  assert_eq!((counted.directories, counted.files), (3, 2));
  let state = app.state.state.current().shares.clone();
  assert_eq!(
    state.hosts.as_deref(),
    Some(&["local".to_string(), "nas".to_string()][..])
  );
  assert_eq!((state.directories, state.files), (5, 3));
  assert_eq!(shares.shared_counts().borrow().files, 3);
  assert_eq!(
    names(shares, "flac").await,
    ["Music\\Other\\02.flac"],
    "the agent's files are searched"
  );
  let found = shares
    .resolve_file("Music\\Other\\02.flac")
    .await
    .unwrap()
    .unwrap();
  assert_eq!((found.host.as_str(), found.size), ("nas", 5));
  let local = shares
    .resolve_file("Music\\Album\\01.mp3")
    .await
    .unwrap()
    .unwrap();
  assert_eq!(
    local.host, "local",
    "a file both hosts have is the local one"
  );
  let browsed = shares.browse().await.unwrap();
  let album = browsed
    .iter()
    .find(|d| d.name.as_str() == "Music\\Album")
    .unwrap();
  assert_eq!(
    album.files.len(),
    2,
    "a directory both hosts have lists both files"
  );
  let by_host = app
    .send(request("GET", "/api/v0/shares", None))
    .await
    .json();
  assert_eq!(by_host["nas"][0]["alias"], listed[0].alias);

  std::fs::write(&dump, b"not a database").unwrap();
  let refused = shares
    .load_host("nas", listed.clone(), &dump)
    .await
    .unwrap_err();
  assert!(matches!(refused, HostError::Invalid(_)), "{refused}");
  assert_eq!(
    names(shares, "flac").await.len(),
    1,
    "a refused upload keeps the host"
  );
  let agent_files = || {
    let mut files: Vec<String> = std::fs::read_dir(&app.state.paths.cache)
      .unwrap()
      .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
      .filter(|name| name.starts_with("shares.nas."))
      .collect();
    files.sort();
    files
  };
  let first = agent_files();
  assert_eq!(
    first.len(),
    usize::from(extra.contains("disk")),
    "{first:?}"
  );
  agent
    .state
    .shares
    .dump(&dump.with_extension("again"))
    .await
    .unwrap();
  shares
    .load_host("nas", listed.clone(), &dump.with_extension("again"))
    .await
    .unwrap();
  assert_eq!(names(shares, "flac").await.len(), 1);
  if extra.contains("disk") {
    eventually(async || {
      let now = agent_files();
      now.len() == 1 && now != first
    })
    .await;
  }

  assert!(shares.remove_host("nas"));
  assert!(!shares.remove_host("nas"));
  eventually(async || agent_files().is_empty()).await;
  assert!(names(shares, "flac").await.is_empty());
  assert_eq!(shares.shared_counts().borrow().files, 1);
  assert_eq!(
    app.state.state.current().shares.hosts.as_deref(),
    Some(&["local".to_string()][..])
  );
}
