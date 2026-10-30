//! What we share: the configured shares, the index a scan builds of them and the watches that
//! keep it current, and the queries other users' searches, browses and downloads are answered
//! from.
//!
//! [`Shares`] is the engine. It owns the local host's index and the indexes relay agents
//! uploaded, runs one scan at a time, keeps `State.shares` current, and offers the queries the
//! network side calls over every host: [`Shares::search`], [`Shares::browse`],
//! [`Shares::folder`], [`Shares::resolve_file`], [`Shares::user_info`] and
//! [`Shares::shared_counts`].

mod attributes;
mod browse;
mod hosts;
mod init;
mod repository;
mod scanner;
mod scanning;
mod search;
mod service;
pub mod share;
mod storage;
mod walk;
mod watcher;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use api_types::state::SharesState;
use cascade::Watcher;
use soulseek_client::LogLimiter;
use soulseek_proto::SearchQuery;
use soulseek_proto::peer::{Directory, File};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, info_span, warn};

use crate::config::Options;
use crate::config::enums::RelayMode;
use crate::db::{Database, DbError};
use crate::paths::Paths;
use crate::state::StateOwner;
use crate::sync::lock;
pub use hosts::{HostError, remove_agent_hosts};
use scanner::{Outcome, Settings};
use share::Share;

/// Failed share queries, which arrive at the network's search rate.
static QUERY_FAILED: LogLimiter = LogLimiter::new();

/// Profile pictures not read, which a peer's every user info request would repeat.
static PICTURE_UNREAD: LogLimiter = LogLimiter::new();

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

/// What our user info says that comes from this host rather than the upload queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalInfo {
  /// the profile text
  pub description: String,
  /// the profile picture's bytes, `None` without one or when it could not be read
  pub picture: Option<Vec<u8>>,
}

/// The name the local host's shares go by.
pub const LOCAL_HOST: &str = "local";

/// Why a scan could not be started or stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanConflict {
  /// a scan is running
  Running,
  /// no scan is running
  NotRunning,
}

/// The share state as this engine keeps it, mirrored into `State.shares`.
#[derive(Clone, Debug, Default, PartialEq)]
struct Status {
  scan_pending: bool,
  scanning: bool,
  ready: bool,
  faulted: bool,
  canceled: bool,
  progress: f64,
  directories: u32,
  files: u32,
  /// the shares were loaded or scanned once, so the host list is known
  loaded: bool,
}

impl Status {
  /// The state with the hosts of the relay agents `agents` and their totals `added`.
  fn to_api(&self, agents: Vec<String>, added: SharedCounts) -> SharesState {
    let listed = self.loaded || !agents.is_empty();
    SharesState {
      scan_pending: self.scan_pending,
      scanning: self.scanning,
      ready: self.ready,
      faulted: self.faulted,
      cancelled: self.canceled,
      scan_progress: self.progress,
      hosts: listed.then(|| {
        std::iter::once(LOCAL_HOST.to_string())
          .chain(agents)
          .collect()
      }),
      directories: self.directories.saturating_add(added.directories),
      files: self.files.saturating_add(added.files),
    }
  }
}

/// An action run at most once a minute: asked for again sooner, it runs once when the minute
/// is up.
#[derive(Default)]
struct Throttle {
  last: Mutex<Option<Instant>>,
  pending: AtomicBool,
}

/// The local host's shares and their index.
pub struct Shares {
  /// the engine itself, for the work it starts in the background
  this: Weak<Shares>,
  db: Database,
  config: Watcher<Options>,
  state: StateOwner,
  /// the cache directory, which holds the browse cache and the index's backup
  cache: PathBuf,
  /// the downloads directory, shared with `shares.share_downloads`
  downloads: PathBuf,
  /// the backup file, `None` when nothing is kept between starts
  backup: Option<PathBuf>,
  in_memory: bool,
  /// a debug relay serves its own files through itself as an agent
  relay_debug: bool,
  workers: u32,
  /// whether the share filters match case, as the daemon started with
  case_sensitive: bool,
  quiet: Duration,
  shutdown: CancellationToken,
  /// the configured shares with their counts
  shares: Mutex<Vec<Share>>,
  /// the share options the configured shares were last made from
  applied: Mutex<crate::config::Shares>,
  /// what the index holds: the settings of the last scan or load
  covered: Mutex<Option<Arc<Settings>>>,
  /// the running scan's cancellation
  running: Mutex<Option<CancellationToken>>,
  status: Mutex<Status>,
  counts: watch::Sender<SharedCounts>,
  /// true once the startup load or scan is done, at once when shares are not scanned at
  /// startup
  initialized: watch::Sender<bool>,
  watches: Mutex<Option<watcher::Watching>>,
  browse_throttle: Throttle,
  counts_throttle: Throttle,
  /// the relay agents' shares, in the order they were loaded
  hosts: Mutex<Vec<hosts::Host>>,
  /// a browse cache rebuild after a host change is waiting
  hosts_settling: AtomicBool,
  /// the local shares' totals as last counted
  local_counts: Mutex<SharedCounts>,
  /// grows each time the local index changed
  changed: watch::Sender<u64>,
}

impl Shares {
  /// Opens the local host's index in the cache directory and starts the shares' background
  /// work: loading or scanning the index, watching the shares, the scheduled rescan and
  /// following the configuration.
  pub async fn open(
    config: Watcher<Options>,
    paths: &Paths,
    state: StateOwner,
    shutdown: CancellationToken,
  ) -> Result<Arc<Self>, DbError> {
    Self::open_with(config, paths, state, shutdown, QUIET).await
  }

  /// [`Shares::open`] with `quiet` as the time a changed path must be quiet before the change
  /// is applied.
  pub(crate) async fn open_with(
    config: Watcher<Options>,
    paths: &Paths,
    state: StateOwner,
    shutdown: CancellationToken,
    quiet: Duration,
  ) -> Result<Arc<Self>, DbError> {
    let startup = config.current();
    let options = &startup.values;
    let storage = storage::open(
      options.shares.cache.storage_mode,
      &paths.cache,
      &paths.backups,
      options.flags.volatile,
    )
    .await?;
    browse::remove_stale(&paths.cache).await;
    remove_agent_hosts(&paths.cache).await;
    let shares = Arc::new_cyclic(|this| Self {
      this: this.clone(),
      db: storage.db,
      shares: Mutex::new(share::configure(&options.shares, &paths.downloads)),
      applied: Mutex::new(options.shares.clone()),
      config: config.clone(),
      state,
      cache: paths.cache.clone(),
      downloads: paths.downloads.clone(),
      backup: storage.backup,
      in_memory: storage.in_memory,
      relay_debug: options.relay.enabled && options.relay.mode == RelayMode::Debug,
      workers: options.shares.cache.workers,
      case_sensitive: options.flags.case_sensitive_regex,
      quiet,
      shutdown,
      covered: Mutex::new(None),
      running: Mutex::new(None),
      status: Mutex::new(Status::default()),
      counts: watch::Sender::new(SharedCounts::default()),
      initialized: watch::Sender::new(options.flags.no_share_scan),
      watches: Mutex::new(None),
      browse_throttle: Throttle::default(),
      counts_throttle: Throttle::default(),
      hosts: Mutex::new(Vec::new()),
      hosts_settling: AtomicBool::new(false),
      local_counts: Mutex::new(SharedCounts::default()),
      changed: watch::Sender::new(0),
    });
    init::start(&shares, options);
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

  /// The files matching an incoming search, the local ones first, then each relay agent's, at
  /// most `limit` from each host; nothing from a host whose index is not loaded.
  pub async fn search(&self, query: &SearchQuery, limit: Option<usize>) -> Vec<File> {
    let text = query.search_text();
    let mut found = Vec::new();
    for db in self.databases() {
      let query = query.clone();
      match db
        .read(move |conn| search::search(conn, &query, limit))
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

  /// Everything we share, the relay agents' shares included, as a browsing peer sees it,
  /// ordered by directory name.
  pub async fn browse(&self) -> Result<Vec<Directory>, DbError> {
    browse::listing(&self.databases(), None).await
  }

  /// The browse cache file, the encoded browse response a peer is sent; rebuilt after every
  /// scan, and by [`Shares::rebuild_browse_cache`] when it is missing.
  pub fn browse_cache(&self) -> PathBuf {
    browse::path(&self.cache)
  }

  /// Writes the browse cache again from the index.
  pub async fn rebuild_browse_cache(&self) -> Result<(), browse::CacheError> {
    browse::rebuild(&self.databases(), &self.browse_cache()).await
  }

  /// The contents of the directory `name` a peer asked `username` for: that directory and each
  /// one under it with its own files. A name with a `.` or `..` part gets nothing.
  pub async fn folder(&self, username: &str, name: &str) -> Result<Vec<Directory>, DbError> {
    if crate::safety::traverses(name) {
      warn!(
        username,
        directory = name,
        "Suspicious attempt to list a directory containing unsafe path segments"
      );
      return Ok(Vec::new());
    }
    let mut listings = Vec::new();
    for db in self.databases() {
      let name = name.to_string();
      listings.push(db.read(move |conn| repository::folder(conn, &name)).await?);
    }
    // a host without the directory lists it empty, which another host's entry fills
    Ok(browse::merge(listings))
  }

  /// Where the file a peer asked for by its exact remote name `masked` is: the local shares
  /// first, then each relay agent's in the order they were loaded; `None` when no host has it.
  /// A debug relay passes over its local file, to serve it through itself as an agent.
  pub async fn resolve_file(&self, masked: &str) -> Result<Option<Resolved>, DbError> {
    if let Some(found) = self.resolve_local(masked).await? {
      if !self.relay_debug {
        return Ok(Some(found));
      }
      warn!(
        filename = masked,
        "Ignoring resolved local file to facilitate Relay debugging."
      );
    }
    for (host, db) in self.agent_databases() {
      let wanted = masked.to_string();
      if let Some((path, size)) = db
        .read(move |conn| repository::find_file(conn, &wanted))
        .await?
      {
        return Ok(Some(Resolved { host, path, size }));
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
    self
      .db
      .write(|conn| repository::mark_latest_suspect(conn))
      .await?;
    self.publish(|status| status.scan_pending = true);
    Ok(())
  }

  /// The profile text and picture from the options; the picture is read each time.
  pub async fn user_info(&self) -> LocalInfo {
    let options = self.config.current();
    let soulseek = &options.values.soulseek;
    let picture = match &soulseek.picture {
      Some(path) => match tokio::fs::read(path).await {
        Ok(bytes) => Some(bytes),
        Err(e) => {
          if let Some(suppressed) = PICTURE_UNREAD.allow() {
            warn!(path = %path.display(), error = %e, suppressed, "profile picture not read");
          }
          None
        }
      },
      None => None,
    };
    LocalInfo {
      description: soulseek.description.clone(),
      picture,
    }
  }

  /// Whether `soulseek.picture` names a file, found without reading it.
  pub async fn has_picture(&self) -> bool {
    let Some(path) = self.config.current().values.soulseek.picture.clone() else {
      return false;
    };
    match tokio::fs::metadata(&path).await {
      Ok(metadata) => metadata.is_file(),
      Err(e) => {
        if let Some(suppressed) = PICTURE_UNREAD.allow() {
          warn!(path = %path.display(), error = %e, suppressed, "profile picture not found");
        }
        false
      }
    }
  }

  /// Changes the share state and mirrors it into `State.shares`.
  fn publish(&self, change: impl FnOnce(&mut Status)) {
    let mut status = lock(&self.status);
    change(&mut status);
    let (agents, added) = self.agent_totals();
    let api = status.to_api(agents, added);
    self.state.update(|state| state.shares = api);
  }

  /// What a scan started now would go by: the configured shares and the current share
  /// options, with the filters matching case as at startup.
  fn scan_settings(&self) -> Settings {
    Settings::new(
      self.listed(),
      &self.config.current().values,
      self.case_sensitive,
    )
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
  /// index and returns the totals. The counts are written back to the configured shares by
  /// entry, so a share added since the index was filled keeps no counts until a scan covers
  /// it.
  async fn count_shares(&self, covering: &[Share]) -> Result<SharedCounts, DbError> {
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
    let mut totals = SharedCounts::default();
    for (directories, files) in counted.values() {
      totals.directories = totals.directories.saturating_add(*directories);
      totals.files = totals.files.saturating_add(*files);
    }
    let mut shares = lock(&self.shares);
    for share in shares.iter_mut() {
      if let Some((directories, files)) = counted.get(&share.raw) {
        share.directories = Some(*directories);
        share.files = Some(*files);
      }
    }
    let api: Vec<_> = shares.iter().map(Share::to_api).collect();
    debug!(shares = ?api, "Share statistics updated");
    Ok(totals)
  }
}

#[cfg(test)]
mod tests;
