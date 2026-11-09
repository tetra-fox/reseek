//! The engine as a whole: scanning at startup and loading the index again, the scan pending
//! after an option change, watched changes, and the indexes of relay agents.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cascade::{Cascade, Layers, Watcher};
use serde::{Deserialize, Serialize};
use soulseek_proto::SearchQuery;
use tokio_util::sync::CancellationToken;

use super::*;

/// The options of the tests: a `[shares]` section alone.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
struct Config {
  /// Shared directories.
  shares: options::Shares,
}

fn shares_of(config: &Config) -> &options::Shares {
  &config.shares
}

/// An engine opened on the configuration file in `dir`, with what it says of its state.
struct Engine {
  /// holds `config.toml` and the cache directory
  dir: tempfile::TempDir,
  config: Watcher<Config>,
  shares: Arc<Shares>,
  state: watch::Receiver<SharesState>,
}

impl Engine {
  /// The share state the engine last told.
  fn state(&self) -> SharesState {
    self.state.borrow().clone()
  }

  fn cache(&self) -> PathBuf {
    self.dir.path().join("cache")
  }

  /// Another engine on the same directory and configuration, as after a restart, with `quiet`
  /// as the time a changed path must be quiet; it is stopped with `shutdown`.
  async fn again(&self, shutdown: CancellationToken, quiet: Duration) -> Arc<Shares> {
    self.again_in(&self.cache(), shutdown, quiet).await
  }

  /// [`Engine::again`] with its index in the cache directory `cache`.
  async fn again_in(
    &self,
    cache: &Path,
    shutdown: CancellationToken,
    quiet: Duration,
  ) -> Arc<Shares> {
    let (setup, _) = setup(self.dir.path(), cache);
    Shares::open_with(&self.config, shares_of, setup, shutdown, quiet)
      .await
      .unwrap()
  }

  /// Rewrites the configuration file to `text` and loads it.
  async fn reconfigure(&self, text: &str) {
    std::fs::write(self.dir.path().join("config.toml"), text).unwrap();
    self.config.reload().await.unwrap();
  }
}

/// What an engine in `dir` with its index in `cache` starts with, and where it tells its state.
fn setup(dir: &Path, cache: &Path) -> (Setup, watch::Receiver<SharesState>) {
  std::fs::create_dir_all(cache).unwrap();
  let (told, state) = watch::channel(Status::default().to_api(Vec::new(), SharedCounts::default()));
  let setup = Setup {
    cache: cache.to_path_buf(),
    backups: dir.join("backups"),
    downloads: dir.join("downloads"),
    scan_at_startup: true,
    force_scan: false,
    volatile: false,
    case_sensitive: false,
    rescan_on_schedule: true,
    scans: Box::new(|_| Box::pin(async {})),
    publish: Box::new(move |shares| {
      told.send_replace(shares);
    }),
  };
  (setup, state)
}

/// An engine whose configuration is `text`, once its startup load or scan is done.
async fn engine(text: &str) -> Engine {
  let dir = tempfile::tempdir().unwrap();
  std::fs::write(dir.path().join("config.toml"), text).unwrap();
  let config = Watcher::new(Layers::<Config>::new().files(dir.path(), ["config.toml"])).unwrap();
  let (setup, state) = setup(dir.path(), &dir.path().join("cache"));
  let shares = Shares::open(&config, shares_of, setup, CancellationToken::new())
    .await
    .unwrap();
  shares.initialized().await;
  Engine {
    dir,
    config,
    shares,
    state,
  }
}

/// A configuration sharing `dir/music` as `Music`, excluding `dir/music/private`.
fn config(dir: &Path) -> String {
  let music = dir.join("music");
  std::fs::create_dir_all(music.join("Album")).unwrap();
  std::fs::create_dir_all(music.join("private")).unwrap();
  std::fs::write(music.join("Album/01.mp3"), "one").unwrap();
  std::fs::write(music.join("private/p.mp3"), "p").unwrap();
  format!(
    "[shares]\ndirectories = [\"[Music]{}\", \"!{}\"]\n",
    music.display(),
    music.join("private").display()
  )
}

async fn shared_engine() -> (tempfile::TempDir, Engine) {
  let files = tempfile::tempdir().unwrap();
  let engine = engine(&config(files.path())).await;
  (files, engine)
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
  names_seen(shares, term, &View::everything()).await
}

/// The files a search for `term` finds that `view` sees.
async fn names_seen(shares: &Shares, term: &str, view: &View) -> Vec<String> {
  let query = SearchQuery {
    terms: vec![term.to_string()],
    exclusions: Vec::new(),
  };
  shares
    .search(&query, None, view)
    .await
    .into_iter()
    .map(|f| f.filename.as_str().to_string())
    .collect()
}

#[test]
fn traversal_is_a_whole_dot_segment() {
  for path in ["foo/../bar", "..", "./x", "/..", ".\\b", "a/."] {
    assert!(traverses(path), "{path}");
  }
  for path in ["foo..bar", ".hidden", "a/..b/c.", "/a/b"] {
    assert!(!traverses(path), "{path}");
  }
}

#[tokio::test]
async fn the_startup_scan_fills_the_index() {
  let (_files, engine) = shared_engine().await;
  let state = engine.state();
  assert!(state.ready && !state.scanning && !state.scan_pending);
  assert_eq!(state.hosts.as_deref(), Some(&["local".to_string()][..]));
  assert_eq!((state.directories, state.files), (2, 1));
  let shares = &engine.shares;
  let listed = shares.local_shares();
  assert_eq!(listed[0].alias, "private");
  assert!(listed[0].is_excluded);
  assert_eq!(listed[0].directories, None);
  assert_eq!(listed[1].alias, "Music");
  assert_eq!(
    listed[1].directories,
    Some(2),
    "the share's own directory counts"
  );
  let contents = shares.share_contents("Music".to_string()).await.unwrap();
  assert_eq!(contents[0].name.as_str(), "Music");
  assert_eq!(contents[1].name.as_str(), "Music\\Album");
  assert_eq!(contents[1].files[0].filename.as_str(), "01.mp3");
  assert_eq!(names(shares, "album").await, ["Music\\Album\\01.mp3"]);
  let found = shares
    .resolve_file("Music\\Album\\01.mp3", &View::everything())
    .await
    .unwrap();
  assert_eq!(found.unwrap().size, 3);
  assert_eq!(
    *shares.shared_counts().borrow(),
    SharedCounts {
      directories: 2,
      files: 1
    }
  );
  assert!(shares.browse_cache(&View::everything()).exists());
}

#[tokio::test]
async fn a_share_hidden_from_a_user_is_answered_as_one_we_do_not_have() {
  let (files, engine) = shared_engine().await;
  let shares = &engine.shares;
  let text = format!(
    "{}[shares.visible_to]\nmusic = [\"buddies\"]\n",
    config(files.path())
  );
  engine.reconfigure(&text).await;
  eventually(async || shares.shared_counts().borrow().files == 0).await;
  let everyone = shares.view(&BTreeSet::new());
  let buddies = shares.view(&BTreeSet::from(["buddies".to_string()]));
  assert!(names_seen(shares, "album", &everyone).await.is_empty());
  assert_eq!(
    names_seen(shares, "album", &buddies).await,
    ["Music\\Album\\01.mp3"]
  );
  let listed = |folder: Vec<Directory>| -> Vec<(String, usize)> {
    folder
      .iter()
      .map(|d| (d.name.as_str().to_string(), d.files.len()))
      .collect()
  };
  let hidden = shares.folder("bob", "Music\\Album", &everyone).await;
  let unknown = shares.folder("bob", "Elsewhere", &buddies).await;
  assert_eq!(listed(hidden.unwrap()), [("Music\\Album".to_string(), 0)]);
  assert_eq!(listed(unknown.unwrap()), [("Elsewhere".to_string(), 0)]);
  let seen = shares.folder("bob", "Music\\Album", &buddies).await;
  assert_eq!(listed(seen.unwrap()), [("Music\\Album".to_string(), 1)]);
  let file = "Music\\Album\\01.mp3";
  assert!(
    shares
      .resolve_file(file, &everyone)
      .await
      .unwrap()
      .is_none()
  );
  assert!(shares.resolve_file(file, &buddies).await.unwrap().is_some());
  let browsed = async |view: &View| {
    shares.rebuild_browse_cache(view).await.unwrap();
    let bytes = std::fs::read(shares.browse_cache(view)).unwrap();
    let soulseek_proto::PeerMessage::BrowseResponse(listing) =
      soulseek_proto::PeerMessage::decode(&bytes).unwrap()
    else {
      panic!("not a browse response");
    };
    listing.directories.len()
  };
  assert_eq!(browsed(&everyone).await, 0);
  assert_eq!(browsed(&buddies).await, 2);
  assert_eq!(
    shares.local_shares()[1].visible_to.as_deref(),
    Some(&["buddies".to_string()][..])
  );

  // who sees the share changes: the views kept are forgotten and their files removed, so none
  // is rewritten for a peer that may no longer have it, nor read stale once one has it again
  let written = shares.browse_cache(&everyone);
  let text = format!(
    "{}[shares.visible_to]\nmusic = [\"family\"]\n",
    config(files.path())
  );
  engine.reconfigure(&text).await;
  eventually(async || !written.exists()).await;
  assert!(lock(&shares.browse_views).kept.is_empty());
  assert_ne!(shares.browse_cache(&everyone), written);
  assert!(shares.browse_cache(&View::everything()).exists());
}

#[tokio::test]
async fn a_scan_is_one_at_a_time_and_only_a_running_one_cancels() {
  let (_files, engine) = shared_engine().await;
  let shares = &engine.shares;
  let held = CancellationToken::new();
  *lock(&shares.running) = Some(held.clone());
  assert_eq!(
    shares.scan(false, ScanOrigin::Asked),
    Err(ScanConflict::Running)
  );
  assert_eq!(shares.cancel_scan(), Ok(()));
  assert!(held.is_cancelled());
  *lock(&shares.running) = None;
  assert_eq!(shares.cancel_scan(), Err(ScanConflict::NotRunning));
  assert_eq!(shares.scan(true, ScanOrigin::Asked), Ok(()));
  eventually(async || lock(&shares.running).is_none()).await;
  assert!(engine.state().ready);
}

#[tokio::test]
async fn filters_only_reordered_or_repeated_mark_no_scan_pending() {
  let (_files, engine) = shared_engine().await;
  let shares = &engine.shares;
  eventually(async || lock(&shares.running).is_none()).await;
  let mut options = engine.config.current().values.shares.clone();
  options.filters = vec!["a$".into(), "b$".into()];
  shares.apply_options(&options);
  // a scan would clear what the first change marked
  shares.publish(|status| status.scan_pending = false);
  options.filters = vec!["b$".into(), "a$".into(), "b$".into()];
  shares.apply_options(&options);
  assert!(!engine.state().scan_pending);
  options.filters = vec!["b$".into()];
  shares.apply_options(&options);
  assert!(engine.state().scan_pending);
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
  let engine = engine(&both).await;
  let shares = engine.shares.clone();
  eventually(async || lock(&shares.running).is_none() && lock(&shares.watches).is_some()).await;
  let watched = |shares: &Shares| {
    lock(&shares.watches)
      .as_ref()
      .map(|w| w.roots.clone())
      .unwrap_or_default()
  };
  assert!(watched(&shares).contains(&other));
  engine.reconfigure(&base).await;
  eventually(async || engine.state().scan_pending).await;
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
  let (files, engine) = shared_engine().await;
  let shares = engine.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  mark_attributes(&shares).await;
  shares
    .db
    .write(|conn| repository::mark_latest_suspect(conn))
    .await
    .unwrap();
  storage::back_up(&shares.db, &storage::backup_path(&engine.cache()))
    .await
    .unwrap();
  let again = engine.again(CancellationToken::new(), QUIET).await;
  again.initialized().await;
  assert_ne!(attributes(&again).await, MARKER);
  drop(files);
}

#[tokio::test]
async fn a_restart_loads_the_backup_and_scans_once() {
  let (files, engine) = shared_engine().await;
  assert!(storage::backup_path(&engine.cache()).exists());
  let again = engine.again(CancellationToken::new(), QUIET).await;
  again.initialized().await;
  assert!(
    lock(&again.running).is_some(),
    "the scan after loading runs in the background"
  );
  eventually(async || lock(&again.running).is_none()).await;
  let scans: i64 = again
    .db
    .read(|conn| conn.query_row("SELECT COUNT(*) FROM scans", [], |row| row.get(0)))
    .await
    .unwrap();
  assert_eq!(scans, 2, "the loaded scan and the one after loading");
  assert_eq!(names(&again, "album").await, ["Music\\Album\\01.mp3"]);
  drop(files);
}

#[tokio::test]
async fn a_scan_holding_the_slot_at_startup_is_not_overwritten_by_a_load() {
  let (files, engine) = shared_engine().await;
  let again = engine.again(CancellationToken::new(), QUIET).await;
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
  let (files, engine) = shared_engine().await;
  let shutdown = CancellationToken::new();
  let shares = engine
    .again_in(
      &files.path().join("cache"),
      shutdown.clone(),
      Duration::from_millis(100),
    )
    .await;
  shares.initialized().await;
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
  let (files, engine) = shared_engine().await;
  let shares = engine.shares.clone();
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
  let (files, engine) = shared_engine().await;
  let shares = engine.shares.clone();
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
  let (files, engine) = shared_engine().await;
  let shares = engine.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  let before = lock(&shares.status).clone();
  assert!(before.ready);
  shares.publish(|status| {
    status.scanning = true;
    status.ready = false;
  });
  shares.faulted("the index refused a write", &before);
  let shown = engine.state();
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
  let engine = engine(&config).await;
  let shares = engine.shares.clone();
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
  let (files, engine) = shared_engine().await;
  let shares = engine
    .again_in(
      &files.path().join("cache"),
      CancellationToken::new(),
      Duration::from_secs(600),
    )
    .await;
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
  let (files, engine) = shared_engine().await;
  let shares = engine.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  let cancel = shares.take_slot().unwrap();
  cancel.cancel();
  assert!(!shares.run_scan(false, ScanOrigin::Asked, cancel).await);
  let shown = engine.state();
  assert!(shown.cancelled && shown.scan_pending && !shown.scanning);
  assert_eq!(shown.scan_progress, 1.0);
  assert!(shown.ready, "the index from before is still served");
  drop(files);
}

/// Adds `dir/other`, holding one file, to the shared directories as `Other`.
fn add_other(engine: &Engine, files: &Path) {
  let other = files.join("other");
  std::fs::create_dir_all(&other).unwrap();
  std::fs::write(other.join("x.mp3"), "x").unwrap();
  let mut options = engine.config.current().values.shares.clone();
  options
    .directories
    .push(format!("[Other]{}", other.display()));
  engine.shares.apply_options(&options);
}

/// The counts of the configured share `alias`.
fn counts(shares: &Shares, alias: &str) -> (Option<u32>, Option<u32>) {
  let listed = shares.listed();
  let share = listed.iter().find(|s| s.alias == alias).unwrap();
  (share.directories, share.files)
}

#[tokio::test]
async fn a_share_added_since_the_last_scan_is_not_counted_by_watched_changes() {
  let (files, engine) = shared_engine().await;
  let shares = engine.shares.clone();
  eventually(async || lock(&shares.running).is_none()).await;
  add_other(&engine, files.path());
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
  let (files, engine) = shared_engine().await;
  let shares = engine.shares.clone();
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
  shares.scan(false, ScanOrigin::Asked).unwrap();
  eventually(async || engine.state().scanning).await;
  add_other(&engine, files.path());
  release.send(()).unwrap();
  hold.await.unwrap().unwrap();
  eventually(async || lock(&shares.running).is_none()).await;
  let shown = engine.state();
  assert!(shown.ready && !shown.scanning);
  assert!(shown.scan_pending, "the scan did not cover the share added");
  assert_eq!(counts(&shares, "Other"), (None, None));
  assert_eq!(counts(&shares, "Music"), (Some(2), Some(1)));
  shares.scan(false, ScanOrigin::Asked).unwrap();
  eventually(async || lock(&shares.running).is_none()).await;
  assert!(!engine.state().scan_pending);
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
  let controller = engine(&(config(files.path()) + extra)).await;
  // the agent's index is another engine's dump, with a share of its own
  let (agent_files, agent) = shared_engine().await;
  std::fs::create_dir_all(agent_files.path().join("music/Other")).unwrap();
  std::fs::write(agent_files.path().join("music/Other/02.flac"), "agent").unwrap();
  agent.shares.scan(false, ScanOrigin::Asked).unwrap();
  eventually(async || lock(&agent.shares.running).is_none() && agent.state().files == 2).await;
  let dump = agent_files.path().join("dump.db");
  agent.shares.dump(&dump).await.unwrap();
  let listed = agent.shares.local_shares();

  let shares = &controller.shares;
  let counted = shares
    .load_host("nas", listed.clone(), &dump)
    .await
    .unwrap();
  assert_eq!((counted.directories, counted.files), (3, 2));
  let state = controller.state();
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
    .resolve_file("Music\\Other\\02.flac", &View::everything())
    .await
    .unwrap()
    .unwrap();
  assert_eq!((found.host.as_str(), found.size), ("nas", 5));
  let local = shares
    .resolve_file("Music\\Album\\01.mp3", &View::everything())
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
  let by_host = shares.agent_shares();
  assert_eq!(by_host[0].0, "nas");
  assert_eq!(by_host[0].1[0].alias, listed[0].alias);

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
    let mut files: Vec<String> = std::fs::read_dir(controller.cache())
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
    controller.state().hosts.as_deref(),
    Some(&["local".to_string()][..])
  );
}
