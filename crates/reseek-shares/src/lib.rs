//! What reseek shares: the configured shares, the index a scan builds of them and the watches
//! that keep it current, and the queries other users' searches, browses and downloads are
//! answered from. The daemon and the relay agent both keep their shares with it.
//!
//! [`Shares`] is the engine. It owns the local host's index and the indexes relay agents
//! uploaded, runs one scan at a time, reports its state as it changes, and offers the queries
//! the network side calls over every host: [`Shares::search`], [`Shares::browse_cache`],
//! [`Shares::folder`], [`Shares::resolve_file`] and [`Shares::shared_counts`]. Each answer to
//! another user is made as the [`View`] of what that user sees, which [`Shares::view`] gives.
//! Its options are the `[shares]` section, [`options::Shares`], checked by [`validate::check`].
//!
//! The crate also holds what the share index is built on and the daemon shares: the SQLite
//! databases ([`db`]), the entries a walk leaves out ([`entries`]), the patterns users write
//! ([`patterns`]), how a loop that keeps failing waits before it tries again ([`backoff`]) and
//! warns about an outage ([`Outage`]), where the configuration files are ([`config_files`]), the
//! line the daemon's and the agent's logs are written in ([`log_format`]), [`lock`] and
//! [`traverses`].

#![warn(missing_docs)]

mod attributes;
pub mod backoff;
mod browse;
pub mod config_files;
pub mod db;
pub mod entries;
mod folders;
mod hosts;
mod init;
pub mod log_format;
pub mod options;
pub mod patterns;
mod phase;
mod repository;
mod scanner;
mod scanning;
pub mod schedule;
mod search;
mod share;
mod storage;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod validate;
mod visibility;
mod walk;
mod watcher;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use api_types::shares::ShareFolders;
use api_types::state::SharesState;
use cascade::Watcher;
use soulseek_client::LogLimiter;
use soulseek_proto::SearchQuery;
use soulseek_proto::peer::{Directory, File};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, info_span, trace, warn};

use crate::db::{Database, DbError};
pub use folders::FolderError;
pub use hosts::HostError;
use phase::{Edge, Phase, ScanResult};
pub use repository::SCHEMA;
pub use reseek_relay_proto::LOCAL_HOST;
use scanner::{Outcome, Settings};
use share::Share;
pub use soulseek_client::Outage;
pub use visibility::View;

/// Failed share queries, which arrive at the network's search rate.
static QUERY_FAILED: LogLimiter = LogLimiter::new();

/// How long a path must be quiet after a change before the change is applied.
const QUIET: Duration = Duration::from_secs(2);

/// How often counts sent to the server and the browse cache follow watched changes.
const AT_MOST_EVERY: Duration = Duration::from_secs(60);

/// The totals the server is told we share.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SharedCounts {
  /// shared directories
  pub directories: u32,
  /// shared files
  pub files: u32,
}

impl SharedCounts {
  fn add(self, other: SharedCounts) -> SharedCounts {
    SharedCounts {
      directories: self.directories.saturating_add(other.directories),
      files: self.files.saturating_add(other.files),
    }
  }
}

/// What each local share holds, by alias, as last counted.
#[derive(Clone, Debug, Default)]
struct Counted(Vec<(String, SharedCounts)>);

impl Counted {
  /// The totals over every share.
  fn total(&self) -> SharedCounts {
    self
      .0
      .iter()
      .fold(SharedCounts::default(), |sum, (_, c)| sum.add(*c))
  }

  /// The totals over the shares `view` sees.
  fn seen_by(&self, view: &View) -> SharedCounts {
    self
      .0
      .iter()
      .filter(|(alias, _)| !view.hides_share(alias))
      .fold(SharedCounts::default(), |sum, (_, c)| sum.add(*c))
  }
}

/// Where a file a peer asked for is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
  /// the host that has it, `local` for this one, else the relay agent's instance name
  pub host: String,
  /// its path on the host that has it
  pub path: PathBuf,
  /// its size when it was indexed
  pub size: u64,
}

/// Why a scan could not be started or stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanConflict {
  /// a scan is running
  Running,
  /// no scan is running
  NotRunning,
}

/// What started a scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanOrigin {
  /// someone asked for it
  Asked,
  /// `shares.cache.retention` found it due
  Scheduled,
  /// the start of the engine: the first scan, or the one that finds what changed while the
  /// program was not running
  Startup,
}

/// How a scan ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanEnded {
  /// every directory was scanned and the index holds these totals
  Completed(SharedCounts),
  /// it was canceled, and the index from before is served
  Canceled,
  /// it failed for this reason, and the index from before is served
  Failed(String),
}

/// A scan starting or ending, as [`Setup::scans`] is told of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanNotice {
  /// a scan started, reading every file again when `force`d
  Started {
    /// whether every file is read again
    force: bool,
    /// what started it
    origin: ScanOrigin,
  },
  /// the scan that started last ended
  Ended(ScanEnded),
}

/// Told of every scan as it starts and as it ends, in that order, before the scan goes on.
pub type ScanHook =
  Box<dyn Fn(ScanNotice) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// The share state as this engine keeps it, told to [`Setup::publish`] as it changes.
#[derive(Debug, Default)]
struct Status {
  /// where the engine is; written only by [`Shares::transition`]
  phase: Phase,
  /// the index may have drifted from the disk or from the share options, so a scan is due; set
  /// and cleared in the database writes it follows
  scan_pending: bool,
}

/// An action run at most once a minute: asked for again sooner, it runs once when the minute
/// is up.
#[derive(Default)]
struct Throttle {
  last: Mutex<Option<Instant>>,
  pending: AtomicBool,
}

/// Where the engine keeps its files and how it starts, fixed for its life.
pub struct Setup {
  /// the cache directory, which holds the index, its backup, the browse cache and the relay
  /// agents' indexes
  pub cache: PathBuf,
  /// where a database is backed up before a migration changes it
  pub backups: PathBuf,
  /// the downloads directory, shared when `shares.share_downloads` asks for it
  pub downloads: PathBuf,
  /// load or scan the index at startup
  pub scan_at_startup: bool,
  /// scan every file at startup, even when the index is current
  pub force_scan: bool,
  /// keep the index in memory only, with no backup
  pub volatile: bool,
  /// the share filters match case
  pub case_sensitive: bool,
  /// scan again when `shares.cache.retention` finds it due; off when the program schedules the
  /// rescans itself through [`Shares::scan`]
  pub rescan_on_schedule: bool,
  /// write the browse caches, the listing a browsing peer is sent; off for a program that
  /// answers no browse
  pub browse_caches: bool,
  /// told of every scan as it starts and ends
  pub scans: ScanHook,
  /// told the share state each time it changes, while the engine's lock on it is held, so the
  /// states arrive in the order they were made
  pub publish: Box<dyn Fn(SharesState) + Send + Sync>,
}

/// The local host's shares and their index.
pub struct Shares {
  /// the engine itself, for the work it starts in the background
  this: Weak<Shares>,
  db: Database,
  /// the share options as they are now
  options: Box<dyn Fn() -> options::Shares + Send + Sync>,
  /// where the share state is told
  state: Box<dyn Fn(SharesState) + Send + Sync>,
  /// told of every scan as it starts and ends
  scans: ScanHook,
  /// the cache directory, which holds the browse cache and the index's backup
  cache: PathBuf,
  /// the downloads directory, shared with `shares.share_downloads`
  downloads: PathBuf,
  /// the backup file, `None` when nothing is kept between starts
  backup: Option<PathBuf>,
  in_memory: bool,
  /// nothing is kept across a stop, agents' indexes included
  volatile: bool,
  workers: u32,
  /// whether the share filters match case, as the engine started with
  case_sensitive: bool,
  /// whether the browse caches are written
  browse_caches: bool,
  quiet: Duration,
  shutdown: CancellationToken,
  /// the configured shares with their counts
  shares: Mutex<Vec<Share>>,
  /// the share options the configured shares were last made from
  applied: Mutex<options::Shares>,
  /// what the index holds: the settings of the last scan or load
  covered: Mutex<Option<Arc<Settings>>>,
  /// shared with the database writes that mark a scan suspect and that settle a scan's end,
  /// so `scan_pending` changes in the same write as the row it follows
  status: Arc<Mutex<Status>>,
  counts: watch::Sender<SharedCounts>,
  /// true once the startup load or scan is done, at once when shares are not scanned at
  /// startup
  initialized: watch::Sender<bool>,
  watches: Mutex<Option<watcher::Watching>>,
  browse_throttle: Throttle,
  counts_throttle: Throttle,
  /// the relay agents' shares, in the order they were loaded
  hosts: Mutex<Vec<hosts::Host>>,
  /// the indexes of the agents that left, by agent, kept to tell whether a file was theirs
  away: Mutex<HashMap<String, hosts::Away>>,
  /// a browse cache rebuild after a host change is waiting
  hosts_settling: AtomicBool,
  /// numbers, in the order they happen, each read of the shares for a browse cache, each
  /// refresh of the browse caches asked for and each time an agent's host came or went
  browse_order: AtomicU64,
  /// the number of the last time an agent's host came or went
  hosts_changed_at: AtomicU64,
  /// held while a browse cache is checked against the one in place and renamed over it, and
  /// while a forgotten generation's files are removed
  browse_placing: Arc<tokio::sync::Mutex<()>>,
  /// what each local share holds as last counted
  local_counts: Mutex<Counted>,
  /// the views other than everything whose browse cache was written, rewritten whenever the
  /// shares change
  browse_views: Mutex<BrowseViews>,
  /// the aliases of the visibility lists that matched no share on any host when last checked
  unmatched: Mutex<HashSet<String>>,
  /// grows each time the local index changed
  changed: watch::Sender<u64>,
}

/// The views that hide something whose browse cache is kept, the generation their files are
/// written in, and the read of the shares each file in place was made from.
#[derive(Default)]
struct BrowseViews {
  kept: BTreeSet<View>,
  generation: u64,
  /// by file, the number of the read it was made from, so a listing read earlier never
  /// replaces one read later
  listed: HashMap<PathBuf, u64>,
}

impl BrowseViews {
  /// Whether the browse cache `file` in place was made from a read numbered after `after`.
  fn read_after(&self, file: &Path, after: u64) -> bool {
    self.listed.get(file).is_some_and(|read| *read > after)
  }
}

impl Shares {
  /// Opens the local host's index in the cache directory and starts the shares' background
  /// work: loading or scanning the index, watching the shares, the scheduled rescan and
  /// following the share options, which `select` finds in the options of `config`.
  pub async fn open<O: cascade::Root>(
    config: &Watcher<O>,
    select: fn(&O) -> &options::Shares,
    setup: Setup,
    shutdown: CancellationToken,
  ) -> Result<Arc<Self>, DbError> {
    Self::open_with(config, select, setup, shutdown, QUIET).await
  }

  /// [`Shares::open`] with `quiet` as the time a changed path must be quiet before the change
  /// is applied.
  pub(crate) async fn open_with<O: cascade::Root>(
    config: &Watcher<O>,
    select: fn(&O) -> &options::Shares,
    setup: Setup,
    shutdown: CancellationToken,
    quiet: Duration,
  ) -> Result<Arc<Self>, DbError> {
    let startup = select(&config.current().values).clone();
    let storage = storage::open(
      startup.cache.storage_mode,
      &setup.cache,
      &setup.backups,
      setup.volatile,
    )
    .await?;
    // before the engine exists no scan can run, so a row still running was cut off by a stop
    init::end_interrupted(&storage.db).await;
    browse::remove_stale(&setup.cache).await;
    let away = hosts::kept_agent_hosts(&setup.cache, !setup.volatile).await;
    let current = config.clone();
    let shares = Arc::new_cyclic(|this| Self {
      this: this.clone(),
      db: storage.db,
      shares: Mutex::new(share::configure(&startup, &setup.downloads)),
      options: Box::new(move || select(&current.current().values).clone()),
      state: setup.publish,
      scans: setup.scans,
      cache: setup.cache,
      downloads: setup.downloads,
      backup: storage.backup,
      in_memory: storage.in_memory,
      volatile: setup.volatile,
      workers: startup.cache.scan_workers(),
      case_sensitive: setup.case_sensitive,
      browse_caches: setup.browse_caches,
      quiet,
      shutdown,
      covered: Mutex::new(None),
      status: Arc::new(Mutex::new(Status::default())),
      counts: watch::Sender::new(SharedCounts::default()),
      initialized: watch::Sender::new(!setup.scan_at_startup),
      watches: Mutex::new(None),
      browse_throttle: Throttle::default(),
      counts_throttle: Throttle::default(),
      hosts: Mutex::new(Vec::new()),
      away: Mutex::new(away),
      hosts_settling: AtomicBool::new(false),
      browse_order: AtomicU64::new(0),
      hosts_changed_at: AtomicU64::new(0),
      browse_placing: Arc::new(tokio::sync::Mutex::new(())),
      local_counts: Mutex::new(Counted::default()),
      browse_views: Mutex::new(BrowseViews::default()),
      unmatched: Mutex::new(HashSet::new()),
      changed: watch::Sender::new(0),
      applied: Mutex::new(startup.clone()),
    });
    init::start(
      &shares,
      config,
      select,
      setup.scan_at_startup,
      setup.force_scan,
      setup.rescan_on_schedule,
    );
    // the startup load or scan checks again; without one this is the only check until a change
    shares.check_unmatched();
    Ok(shares)
  }

  /// Waits until the shares are loaded or scanned at startup, so the client can log in with
  /// them in place; returns at once when they are not scanned at startup.
  pub async fn initialized(&self) {
    let mut done = self.initialized.subscribe();
    // the sender is the engine's own field, so it lives as long as this call can
    drop(done.wait_for(|done| *done).await);
  }

  /// The local host's index, for the list of databases.
  pub fn database(&self) -> Database {
    self.db.clone()
  }

  /// The totals of what we share, current from the last scan, load or watched change; the
  /// server is told them at login and whenever they change. Watched changes move them at most
  /// once a minute.
  pub fn shared_counts(&self) -> watch::Receiver<SharedCounts> {
    self.counts.subscribe()
  }

  /// What a member of `groups` sees, under the share options in effect.
  pub fn view(&self, groups: &BTreeSet<String>) -> View {
    View::of(&lock(&self.applied), groups)
  }

  /// Warns of each alias of `shares.visible_to` that no share on any host has, when it comes
  /// to have none, and says so when it comes to have one: a share meant to be hidden whose
  /// alias is spelled otherwise is seen by everyone. Run whenever the option or the hosts'
  /// shares change.
  fn check_unmatched(&self) {
    let listed: Vec<String> = lock(&self.applied).visible_to.keys().cloned().collect();
    let now: HashSet<String> = listed
      .into_iter()
      .filter(|alias| !self.has_alias(&alias.to_lowercase()))
      .collect();
    let mut before = lock(&self.unmatched);
    for alias in now.difference(&before) {
      warn!(
        alias,
        "a share listed in shares.visible_to matches no share's alias on any host; it hides \
         nothing"
      );
    }
    for alias in before.difference(&now) {
      info!(
        alias,
        "a share listed in shares.visible_to now matches a share"
      );
    }
    *before = now;
  }

  /// The files matching an incoming search that `view` sees, the local ones first, then each
  /// relay agent's, at most `limit` from each host; nothing from a host whose index is not
  /// loaded.
  pub async fn search(&self, query: &SearchQuery, limit: Option<usize>, view: &View) -> Vec<File> {
    let text = query.search_text();
    let mut found = Vec::new();
    for db in self.databases() {
      let (query, view) = (query.clone(), view.clone());
      match db
        .read(move |conn| search::search(conn, &query, limit, &view))
        .await
      {
        Ok(files) => found.extend(files),
        Err(e) => {
          if let Some(suppressed) = QUERY_FAILED.allow() {
            warn!(query = text, error = %e, suppressed, "Failed to execute shared file query");
          }
        }
      }
    }
    found
  }

  /// Everything we share, the relay agents' shares included, hiding none, ordered by directory
  /// name.
  pub async fn browse(&self) -> Result<Vec<Directory>, DbError> {
    browse::listing(&self.databases(), None).await
  }

  /// What the share with the alias `alias` holds on every host, as a browsing peer sees it,
  /// its own directory included.
  pub async fn share_contents(&self, alias: String) -> Result<Vec<Directory>, DbError> {
    browse::listing(&self.databases(), Some(alias)).await
  }

  /// The folders of this machine a share could be made of, directly in `path` or at the top
  /// without one: never a file, and nothing a scan would leave out.
  pub async fn folders(&self, path: Option<String>) -> Result<ShareFolders, FolderError> {
    let settings = self.scan_settings();
    tokio::task::spawn_blocking(move || folders::list(&settings, path.as_deref()))
      .await
      .map_err(|e| FolderError::Failed(e.to_string()))?
  }

  /// The browse cache file of `view` when one is in place that was read after an agent's host
  /// last came or went; `None` when the view's file is still to be written, for this view or
  /// since that change.
  pub fn current_browse_cache(&self, view: &View) -> Option<PathBuf> {
    let changed = self.hosts_changed_at.load(Ordering::Acquire);
    let views = lock(&self.browse_views);
    let file = browse::path(&self.cache, view, views.generation);
    views.read_after(&file, changed).then_some(file)
  }

  /// The browse cache file of `view`, the encoded browse response a peer with that view is
  /// sent; written by [`Shares::rebuild_browse_cache`] when it is missing, and rewritten
  /// whenever the shares change.
  pub fn browse_cache(&self, view: &View) -> PathBuf {
    browse::path(&self.cache, view, lock(&self.browse_views).generation)
  }

  /// Writes the browse cache of `view` again from the index, unless one read later is put in
  /// place first; returns the file.
  pub async fn rebuild_browse_cache(&self, view: &View) -> Result<PathBuf, browse::CacheError> {
    // the view is kept in the generation its file is written in, so the file is rewritten
    // with the shares for as long as it can be read
    let file = {
      let mut views = lock(&self.browse_views);
      if !view.is_everything() {
        views.kept.insert(view.clone());
      }
      browse::path(&self.cache, view, views.generation)
    };
    self.rewrite_browse_cache(view, &file).await?;
    Ok(file)
  }

  /// Writes the browse cache of `view` at `file` from the shares as they are now, unless one
  /// read later is put in place first.
  async fn rewrite_browse_cache(&self, view: &View, file: &Path) -> Result<(), browse::CacheError> {
    let started = Instant::now();
    debug!("Warming browse response cache...");
    let written = self.write_browse_cache(view, file).await?;
    if self.place_browse_cache(view, written).await? {
      info!(
        ms = started.elapsed().as_millis() as u64,
        "Browse response cached successfully"
      );
    }
    Ok(())
  }

  /// The browse cache of `view` for `file`, written beside it from a read of the shares
  /// numbered as it starts.
  async fn write_browse_cache(
    &self,
    view: &View,
    file: &Path,
  ) -> Result<browse::Unplaced, browse::CacheError> {
    // numbered before the hosts are taken, so every host change numbered before it is in it
    let read = self.next_in_order();
    browse::write(&self.databases(), file, view, read).await
  }

  /// Puts `written` in place of its file unless the file holds a listing read after it, which
  /// is left as it is, or belongs to a generation forgotten since it was written; returns
  /// whether it was put in place.
  async fn place_browse_cache(
    &self,
    view: &View,
    written: browse::Unplaced,
  ) -> Result<bool, std::io::Error> {
    // checked and renamed under one lock, so no later listing is put in place between the two,
    // and the removal of a forgotten generation's files runs before the check or after the rename
    let _placing = self.browse_placing.lock().await;
    let (forgotten, read_later) = {
      let views = lock(&self.browse_views);
      (
        browse::path(&self.cache, view, views.generation) != written.file,
        views.read_after(&written.file, written.read),
      )
    };
    if forgotten || read_later {
      debug!(forgotten, "browse cache discarded");
      written.discard().await;
      return Ok(false);
    }
    written.place().await?;
    let mut views = lock(&self.browse_views);
    // a generation forgotten since the check removes the file once the lock is let go
    if browse::path(&self.cache, view, views.generation) == written.file {
      views.listed.insert(written.file, written.read);
    }
    Ok(true)
  }

  /// The next number in the order of browse cache reads and host changes.
  fn next_in_order(&self) -> u64 {
    self.browse_order.fetch_add(1, Ordering::AcqRel) + 1
  }

  /// Rewrites the browse cache of every view a peer browsed with, and of the view that hides
  /// nothing, after the shares changed, leaving a file read since it was asked for; a failure
  /// is logged.
  async fn refresh_browse_caches(&self) {
    if !self.browse_caches {
      return;
    }
    let asked = self.next_in_order();
    let files: Vec<(View, PathBuf)> = {
      let views = lock(&self.browse_views);
      std::iter::once(View::everything())
        .chain(views.kept.iter().cloned())
        .map(|view| {
          let file = browse::path(&self.cache, &view, views.generation);
          (view, file)
        })
        .collect()
    };
    for (view, file) in files {
      if lock(&self.browse_views).read_after(&file, asked) {
        continue;
      }
      if let Err(e) = self.rewrite_browse_cache(&view, &file).await {
        error!(error = %e, "Error caching browse response");
      }
    }
  }

  /// Forgets every view that hides something, after who sees which share changed, through the
  /// shares' visibility or a user's groups: one a peer still has is written again at its next
  /// browse, and the files of the others are removed.
  pub fn forget_browse_views(&self) {
    let everything = browse::path(&self.cache, &View::everything(), 0);
    let generation = {
      let mut views = lock(&self.browse_views);
      views.kept.clear();
      views.listed.retain(|file, _| *file == everything);
      views.generation += 1;
      views.generation
    };
    let (cache, placing) = (self.cache.clone(), self.browse_placing.clone());
    // after a placement under way, which checked the generation before it moved on
    let removal = async move {
      let _placing = placing.lock().await;
      browse::remove_older(&cache, generation).await;
    };
    tokio::spawn(removal.in_current_span());
  }

  /// The contents of the directory `name` a peer asked `username` for, as `view` sees it: that
  /// directory and each one under it with its own files. A name with a `.` or `..` part gets
  /// nothing; a directory `view` hides is answered as one we do not share.
  pub async fn folder(
    &self,
    username: &str,
    name: &str,
    view: &View,
  ) -> Result<Vec<Directory>, DbError> {
    if traverses(name) {
      warn!(
        username,
        directory = name,
        "Suspicious attempt to list a directory containing unsafe path segments"
      );
      return Ok(Vec::new());
    }
    if view.hides(name) {
      trace!(
        username,
        directory = name,
        "folder of a share hidden from the user"
      );
      return Ok(vec![Directory {
        name: name.to_string().into(),
        files: Vec::new(),
      }]);
    }
    let mut listings = Vec::new();
    for db in self.databases() {
      let name = name.to_string();
      match db.read(move |conn| repository::folder(conn, &name)).await {
        Ok(listing) => listings.push(listing),
        Err(e) => db.left_out(e)?,
      }
    }
    // a host without the directory lists it empty, which another host's entry fills
    Ok(browse::merge(listings))
  }

  /// Where the file a peer asked for by its exact remote name `masked` is: the local shares
  /// first, then each relay agent's in the order they were loaded, leaving out an agent's index
  /// that cannot be read; `None` when no host has it or `view` hides it.
  pub async fn resolve_file(&self, masked: &str, view: &View) -> Result<Option<Resolved>, DbError> {
    if view.hides(masked) {
      return Ok(None);
    }
    if let Some(found) = self.resolve_local(masked).await? {
      return Ok(Some(found));
    }
    for (host, db) in self.agent_databases() {
      let wanted = masked.to_string();
      match db
        .read(move |conn| repository::find_file(conn, &wanted))
        .await
      {
        Ok(Some((path, size))) => return Ok(Some(Resolved { host, path, size })),
        Ok(None) => {}
        Err(e) => db.left_out(e)?,
      }
    }
    Ok(None)
  }

  /// Where the file `masked` is among the local shares, `None` when no local share has it.
  pub async fn resolve_local(&self, masked: &str) -> Result<Option<Resolved>, DbError> {
    let masked = masked.to_string();
    let found = self
      .db
      .read(move |conn| repository::find_file(conn, &masked))
      .await?;
    Ok(found.map(|(path, size)| Resolved {
      host: LOCAL_HOST.to_string(),
      path,
      size,
    }))
  }

  /// Marks the index as drifted from the disk, such as after a shared file was found missing
  /// or changed during an upload, so a rescan is pending and the next start scans again.
  pub async fn request_scan(&self) -> Result<(), DbError> {
    let status = self.status.clone();
    self
      .db
      .write(move |conn| {
        repository::mark_latest_suspect(conn)?;
        lock(&status).scan_pending = true;
        Ok(())
      })
      .await?;
    self.publish();
    Ok(())
  }

  /// Tells the share state, as it stands, to whoever the engine was opened for.
  fn publish(&self) {
    self.publish_with(&lock(&self.status));
  }

  /// Tells `status` while its lock is held, so the states arrive in the order they were made.
  fn publish_with(&self, status: &Status) {
    let (agents, added) = self.agent_totals();
    (self.state)(status.phase.to_api(status.scan_pending, agents, added));
  }

  /// Moves the engine's phase along `edge`, tells the new state and returns the phase it left;
  /// `None`, changing nothing, when the phase does not take that edge. The one writer of the
  /// phase.
  fn transition(&self, edge: Edge) -> Option<Phase> {
    let mut status = lock(&self.status);
    let next = status.phase.after(&edge)?;
    if matches!(
      edge,
      Edge::ScanEnded(ScanResult::Canceled | ScanResult::Failed)
    ) {
      status.scan_pending = true;
    }
    let left = std::mem::replace(&mut status.phase, next);
    self.publish_with(&status);
    Some(left)
  }

  /// Marks a rescan pending and tells the state.
  fn mark_pending(&self) {
    let mut status = lock(&self.status);
    status.scan_pending = true;
    self.publish_with(&status);
  }

  /// What a scan started now would go by: the configured shares and the current share
  /// options, with the filters matching case as at startup.
  fn scan_settings(&self) -> Settings {
    Settings::new(self.listed(), &(self.options)(), self.case_sensitive)
  }

  /// The configured shares with their counts.
  fn listed(&self) -> Vec<Share> {
    lock(&self.shares).clone()
  }

  /// The settings the index was last filled from by a completed scan or load.
  fn filled_from(&self) -> Arc<Settings> {
    lock(&self.covered)
      .clone()
      .expect("shares are watched only once a scan or load has filled the index")
  }

  /// Counts what each share of `covering`, the shares the index was filled from, holds in the
  /// index and returns the counts by alias. The counts are written back to the configured
  /// shares by entry, so a share added since the index was filled keeps no counts until a scan
  /// covers it.
  async fn count_shares(&self, covering: &[Share]) -> Result<Counted, DbError> {
    let counting: Vec<(String, String)> = covering
      .iter()
      .filter(|s| !s.excluded)
      .map(|s| (s.raw.clone(), s.alias.clone()))
      .collect();
    let counted = self
      .db
      .read(move |conn| {
        counting
          .into_iter()
          .map(|(raw, alias)| Ok((raw, repository::count(conn, Some(&alias))?)))
          .collect::<rusqlite::Result<HashMap<_, _>>>()
      })
      .await?;
    let by_alias = Counted(
      covering
        .iter()
        .filter_map(|share| {
          let (directories, files) = *counted.get(&share.raw)?;
          Some((share.alias.clone(), SharedCounts { directories, files }))
        })
        .collect(),
    );
    let options = lock(&self.applied).clone();
    let mut shares = lock(&self.shares);
    for share in shares.iter_mut() {
      if let Some((directories, files)) = counted.get(&share.raw) {
        share.directories = Some(*directories);
        share.files = Some(*files);
      }
    }
    let api: Vec<_> = shares
      .iter()
      .map(|share| share.to_api(visibility::of(&options, &share.alias)))
      .collect();
    debug!(shares = ?api, "Share statistics updated");
    Ok(by_alias)
  }

  /// What the server is told we share: the totals over the shares everyone sees, the local
  /// ones as last counted and each relay agent's.
  fn server_counts(&self) -> SharedCounts {
    let view = View::everyone(&lock(&self.applied));
    lock(&self.local_counts)
      .seen_by(&view)
      .add(self.agent_counts(&view))
  }

  /// Tells the server's counts again when they moved, such as after a share became hidden
  /// from everyone.
  fn recount_for_server(&self) {
    let now = self.server_counts();
    self.counts.send_if_modified(|sent| {
      let changed = *sent != now;
      *sent = now;
      changed
    });
  }
}

/// Takes `mutex`, also after a thread panicked while holding it. Every lock is changed whole
/// before it is let go, so what a panic leaves behind is a value as sound as any.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether a segment of `path`, split on `/` and `\`, is `.` or `..`.
pub fn traverses(path: &str) -> bool {
  path.split(['/', '\\']).any(|s| s == "." || s == "..")
}

#[cfg(test)]
mod tests;
