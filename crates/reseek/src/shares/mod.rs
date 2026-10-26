//! What we share: the configured shares, the index a scan builds of them and the watches that
//! keep it current, and the queries other users' searches, browses and downloads are answered
//! from.
//!
//! [`Shares`] is the engine. It owns the local host's index, runs one scan at a time, keeps
//! `State.shares` current, and offers the queries the network side calls: [`Shares::search`],
//! [`Shares::browse`], [`Shares::folder`], [`Shares::resolve_file`], [`Shares::user_info`] and
//! [`Shares::shared_counts`].

mod attributes;
mod browse;
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
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use api_types::state::SharesState;
use cascade::Watcher;
use soulseek_proto::SearchQuery;
use soulseek_proto::peer::{Directory, File};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, info_span, warn};

use crate::config::Options;
use crate::db::{Database, DbError};
use crate::paths::Paths;
use crate::state::StateOwner;
use scanner::{Outcome, Settings};
use share::Share;

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
  /// the host that has it, `local` for this one
  pub host: &'static str,
  /// its local path
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
  cancelled: bool,
  progress: f64,
  directories: u32,
  files: u32,
  /// the shares were loaded or scanned once, so the host list is known
  loaded: bool,
}

impl Status {
  fn to_api(&self) -> SharesState {
    SharesState {
      scan_pending: self.scan_pending,
      scanning: self.scanning,
      ready: self.ready,
      faulted: self.faulted,
      cancelled: self.cancelled,
      scan_progress: self.progress,
      hosts: self.loaded.then(|| vec![LOCAL_HOST.to_string()]),
      directories: self.directories,
      files: self.files,
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
  workers: u32,
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
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  // every change under these locks is made whole before the lock is let go
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
    let browse = browse::path(&paths.cache);
    match std::fs::remove_file(&browse) {
      Ok(()) => debug!("browse cache of the last run removed"),
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
      Err(e) => warn!(error = %e, "browse cache of the last run not removed"),
    }
    let shares = Arc::new_cyclic(|this| Self {
      this: this.clone(),
      db: storage.db,
      shares: Mutex::new(share::configure(options, &paths.downloads)),
      applied: Mutex::new(options.shares.clone()),
      config: config.clone(),
      state,
      cache: paths.cache.clone(),
      downloads: paths.downloads.clone(),
      backup: storage.backup,
      in_memory: storage.in_memory,
      workers: options.shares.cache.workers,
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

  /// The files matching an incoming search, local first, at most `limit` of them; nothing when
  /// the index is not loaded.
  pub async fn search(&self, query: &SearchQuery, limit: Option<usize>) -> Vec<File> {
    let text = query.search_text();
    let query = query.clone();
    match self
      .db
      .read(move |conn| Ok(search::search(conn, &query, limit)))
      .await
    {
      Ok(files) => files,
      Err(e) => {
        if let Some(suppressed) = search::QUERY_FAILED.allow() {
          warn!(query = text, error = %e, suppressed, "Failed to execute shared file query");
        }
        Vec::new()
      }
    }
  }

  /// Everything we share as a browsing peer sees it, ordered by directory name.
  pub async fn browse(&self) -> Result<Vec<Directory>, DbError> {
    self.db.read(|conn| repository::browse(conn, None)).await
  }

  /// The browse cache file, the encoded browse response a peer is sent; rebuilt after every
  /// scan, and by [`Shares::rebuild_browse_cache`] when it is missing.
  pub fn browse_cache(&self) -> PathBuf {
    browse::path(&self.cache)
  }

  /// Writes the browse cache again from the index.
  pub async fn rebuild_browse_cache(&self) -> Result<(), browse::CacheError> {
    browse::rebuild(&self.db, &self.browse_cache()).await
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
    let name = name.to_string();
    self
      .db
      .read(move |conn| repository::folder(conn, &name))
      .await
  }

  /// Where the file a peer asked for by its exact remote name `masked` is, `None` when no
  /// share has it.
  pub async fn resolve_file(&self, masked: &str) -> Result<Option<Resolved>, DbError> {
    let masked = masked.to_string();
    let found = self
      .db
      .read(move |conn| repository::find_file(conn, &masked))
      .await?;
    Ok(found.map(|(path, size)| Resolved {
      host: LOCAL_HOST,
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
          warn!(path = %path.display(), error = %e, "profile picture not read");
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

  /// Changes the share state and mirrors it into `State.shares`.
  fn publish(&self, change: impl FnOnce(&mut Status)) {
    let mut status = lock(&self.status);
    change(&mut status);
    let api = status.to_api();
    self.state.update(|state| state.shares = api);
  }

  /// The configured shares with their counts.
  fn listed(&self) -> Vec<Share> {
    lock(&self.shares).clone()
  }

  /// Counts what each configured share holds in the index and returns the totals. The counts
  /// are written back to the shares by entry, so a list replaced while the index was read
  /// keeps its own counts.
  async fn count_shares(&self) -> Result<SharedCounts, DbError> {
    let counting: Vec<(String, String)> = self
      .listed()
      .into_iter()
      .filter(|s| !s.excluded)
      .map(|s| (s.raw, s.alias))
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
