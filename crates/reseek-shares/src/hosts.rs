//! The shares of relay agents. A controller loads the index each of its agents uploads as a
//! host of its own beside the local one; the queries peers are answered from then cover every
//! host, the local one first, and the totals and the browse cache count them all. A host is
//! loaded whole, from a file checked first, and goes the moment its agent does. On disk each
//! upload is a file of its own, which the host's index is swapped to; the file it replaces is
//! deleted once the last query reading it has let go of it, so no file is ever replaced while
//! it is open. The index of an agent that left is kept away from the queries, only to tell
//! whether a file was its, until the agent loads a new one or is forgotten; on disk it is kept
//! across a stop and found again at the next start.

use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_types::shares::Share as ApiShare;
use soulseek_client::LogLimiter;
use tracing::{debug, info, warn};

use super::phase::Phase;
use super::{LOCAL_HOST, SharedCounts, Shares, View, repository, storage, visibility};
use crate::db::{Database, DbError, Location, remove_database};
use crate::lock;

/// How long the browse caches wait after an agent's host changed before they are rebuilt, so
/// an agent that comes and goes rebuilds them once.
const HOST_SETTLE: Duration = Duration::from_secs(2);

/// Numbers each agent index file this process writes, so none is written over another.
static HOST_FILES: AtomicU64 = AtomicU64::new(0);

/// Agents' indexes that failed a query of every host, which peers make at their own rate.
static HOST_UNREAD: LogLimiter = LogLimiter::new();

/// An index the queries read: the local one, or an agent's, whose file, when it has one of its
/// own, is deleted when the last reader lets go of it unless it was kept as an away index.
pub(super) struct Index {
  /// `None` only while it is dropped
  db: Option<Database>,
  file: Mutex<Option<PathBuf>>,
  /// the agent whose index it is, `None` for the local one
  agent: Option<String>,
}

impl Index {
  /// The local index, whose file stays.
  pub(super) fn local(db: Database) -> Arc<Self> {
    Arc::new(Self {
      db: Some(db),
      file: Mutex::new(None),
      agent: None,
    })
  }

  /// What a query of every host does when this index failed it with `e`: an agent's index is
  /// logged and left out, so it never takes the other hosts down, the local one's failure is
  /// returned.
  pub(super) fn left_out(&self, e: DbError) -> Result<(), DbError> {
    let Some(agent) = &self.agent else {
      return Err(e);
    };
    if let Some(suppressed) = HOST_UNREAD.allow() {
      warn!(agent, error = %e, suppressed, "agent's share index not read, its shares left out");
    }
    Ok(())
  }
}

impl Deref for Index {
  type Target = Database;

  fn deref(&self) -> &Database {
    self
      .db
      .as_ref()
      .expect("the database is taken only when the index is dropped")
  }
}

impl Drop for Index {
  fn drop(&mut self) {
    // the database's connections close first, so the file is free to delete on any system
    drop(self.db.take());
    if let Some(file) = lock(&self.file).take() {
      debug!(file = %file.display(), "replaced agent share database deleted");
      delete(file);
    }
  }
}

/// Deletes the database `file`, on a blocking thread when there is a runtime.
fn delete(file: PathBuf) {
  match tokio::runtime::Handle::try_current() {
    // the deletion runs on its own and logs its own failure
    Ok(runtime) => drop(runtime.spawn_blocking(move || remove_database(&file))),
    Err(_) => remove_database(&file),
  }
}

/// The index of an agent that left. Dropped, its file stays, so one held when the process stops
/// is found at the next start; [`Away::discard`] deletes it.
pub(super) struct Away {
  index: Arc<Index>,
  /// the index's file, `None` for one in memory
  file: Option<PathBuf>,
}

impl Away {
  /// The index `index` of an agent that left, its file taken from it so it stays.
  fn of(index: Arc<Index>) -> Self {
    let file = lock(&index.file).take();
    Self { index, file }
  }

  /// Lets go of the index and deletes its file.
  fn discard(self) {
    drop(self.index);
    if let Some(file) = self.file {
      debug!(file = %file.display(), "agent's kept share database deleted");
      delete(file);
    }
  }
}

/// An agent's shares as loaded.
pub(super) struct Host {
  /// the agent's instance name
  pub name: String,
  /// the shares it listed, with its counts
  pub shares: Vec<ApiShare>,
  pub index: Arc<Index>,
  /// directories over its shares, as it counted them
  pub directories: u32,
  /// files over its shares, as it counted them
  pub files: u32,
}

/// Why an agent's index was not loaded.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
  /// the uploaded file is not an index this program writes
  #[error("Invalid database: {0}")]
  Invalid(String),
  /// it could not be moved into place or opened
  #[error("the agent's share database could not be loaded: {0}")]
  Failed(String),
}

impl From<DbError> for HostError {
  fn from(e: DbError) -> Self {
    Self::Failed(e.to_string())
  }
}

/// The totals over `shares`, as an agent counted them.
fn totals(shares: &[ApiShare]) -> SharedCounts {
  shares
    .iter()
    .fold(SharedCounts::default(), |sum, share| SharedCounts {
      directories: sum
        .directories
        .saturating_add(share.directories.unwrap_or(0)),
      files: sum.files.saturating_add(share.files.unwrap_or(0)),
    })
}

/// A new index file for the host `name` in the cache directory `cache`.
fn host_file(cache: &Path, name: &str) -> PathBuf {
  let number = HOST_FILES.fetch_add(1, Ordering::Relaxed);
  cache.join(format!("shares.{name}.{number}.db"))
}

impl Shares {
  /// The version of the share index's schema, which an agent's must equal.
  pub fn schema_version() -> u32 {
    storage::schema_version()
  }

  /// Writes a consistent copy of the local index to `path`, in place of a file left there, and
  /// returns its size.
  pub async fn dump(&self, path: &Path) -> Result<u64, DbError> {
    storage::dump(&self.db, path).await
  }

  /// About the size [`Shares::dump`] would write now, read without writing anything.
  pub async fn index_bytes(&self) -> Result<u64, DbError> {
    storage::used_bytes(&self.db).await
  }

  /// The local shares as the API shows them, each with its visibility.
  pub fn local_shares(&self) -> Vec<ApiShare> {
    let options = lock(&self.applied).clone();
    self
      .listed()
      .iter()
      .map(|share| share.to_api(visibility::of(&options, &share.alias)))
      .collect()
  }

  /// Whether the index is being loaded at startup or a scan of the local shares is running,
  /// which is what the share state reports as scanning. A scan that ended while the scan hook
  /// is told of its end is neither: the index it filled is complete.
  pub fn loading_or_scanning(&self) -> bool {
    matches!(
      lock(&self.status).phase,
      Phase::Loading { .. } | Phase::Scanning { .. }
    )
  }

  /// Whether the index served was filled and counted by the startup load or a completed scan.
  /// A scan running over such an index leaves it served, and this true, until it ends.
  pub fn index_filled(&self) -> bool {
    lock(&self.status).phase.filled()
  }

  /// A number that grows each time the local index changed: a scan completed, the index was
  /// loaded, or watched changes were applied, these at most once a minute. The load's change is
  /// told once the loaded index is served, as the scan that follows it starts.
  pub fn changes(&self) -> tokio::sync::watch::Receiver<u64> {
    self.changed.subscribe()
  }

  /// A new file in the cache directory for the index the agent `name` uploads, which
  /// [`Shares::load_host`] renames into place unless nothing is kept across a stop; one a run
  /// left behind is deleted at the next start.
  pub fn upload_file(&self, name: &str) -> PathBuf {
    let number = HOST_FILES.fetch_add(1, Ordering::Relaxed);
    self.cache.join(format!("shares.{name}.{number}.db.tmp"))
  }

  /// Loads the index an agent uploaded to `upload`, a [`Shares::upload_file`], as the host
  /// `name` with its `shares`, in place of the one it had. The file is checked first; on disk
  /// it then becomes the host's index file, in memory it is copied and kept as the host's file
  /// for the next start, unless nothing is kept across a stop, when it stays for the caller to
  /// delete. The host is replaced only once the new index is ready. Returns the totals the agent
  /// counted.
  pub async fn load_host(
    self: &Arc<Self>,
    name: &str,
    shares: Vec<ApiShare>,
    upload: &Path,
  ) -> Result<SharedCounts, HostError> {
    storage::check_foreign(upload)
      .await
      .map_err(HostError::Invalid)?;
    info!(agent = name, "Loading shares from agent");
    let index = if self.in_memory {
      let scratch = std::env::temp_dir();
      let location = Location {
        data: scratch.clone(),
        backups: scratch,
        volatile: true,
        log_sql: false,
      };
      let db = Database::open_as(&repository::SCHEMA, &location, &format!("shares.{name}")).await?;
      storage::restore(&db, upload).await?;
      let kept = if self.volatile {
        None
      } else {
        self.keep_upload(name, upload).await
      };
      Arc::new(Index {
        db: Some(db),
        file: Mutex::new(kept),
        agent: Some(name.to_string()),
      })
    } else {
      let target = host_file(&self.cache, name);
      let (from, to) = (upload.to_path_buf(), target.clone());
      tokio::task::spawn_blocking(move || storage::move_host_file(&from, &to))
        .await
        .map_err(|e| HostError::Failed(e.to_string()))?
        .map_err(HostError::Failed)?;
      let db = match Database::open_immutable(&repository::SCHEMA, &target).await {
        Ok(db) => db,
        Err(e) => {
          let unused = target.clone();
          // a file that would not open is nobody's
          drop(tokio::task::spawn_blocking(move || {
            remove_database(&unused)
          }));
          return Err(e.into());
        }
      };
      Arc::new(Index {
        db: Some(db),
        file: Mutex::new(Some(target)),
        agent: Some(name.to_string()),
      })
    };
    let counted = totals(&shares);
    let host = Host {
      name: name.to_string(),
      shares,
      index,
      directories: counted.directories,
      files: counted.files,
    };
    {
      let mut hosts = lock(&self.hosts);
      match hosts.iter_mut().find(|h| h.name == name) {
        Some(loaded) => *loaded = host,
        None => hosts.push(host),
      }
    }
    if let Some(away) = lock(&self.away).remove(name) {
      away.discard();
    }
    self.hosts_changed();
    Ok(counted)
  }

  /// The upload `upload` of the agent `name` as its host's file, for when the agent is away at
  /// the next start; `None` when it could not be moved, so nothing is kept.
  async fn keep_upload(&self, name: &str, upload: &Path) -> Option<PathBuf> {
    let target = host_file(&self.cache, name);
    let (from, to) = (upload.to_path_buf(), target.clone());
    let moved = tokio::task::spawn_blocking(move || storage::move_host_file(&from, &to))
      .await
      .map_err(|e| e.to_string())
      .and_then(|moved| moved);
    match moved {
      Ok(()) => Some(target),
      Err(e) => {
        // the agent's files are then not known while it is away after a restart
        warn!(agent = name, error = %e, "agent's share database not kept for the next start");
        None
      }
    }
  }

  /// Drops the host `name`, whose agent left, keeping its index away from the queries; false
  /// when it had none.
  pub fn remove_host(self: &Arc<Self>, name: &str) -> bool {
    let removed = {
      let mut hosts = lock(&self.hosts);
      hosts
        .iter()
        .position(|h| h.name == name)
        .map(|at| hosts.remove(at))
    };
    let Some(host) = removed else {
      return false;
    };
    debug!(agent = name, "agent's shares unloaded");
    let replaced = lock(&self.away).insert(name.to_string(), Away::of(host.index));
    if let Some(replaced) = replaced {
      replaced.discard();
    }
    self.hosts_changed();
    true
  }

  /// The agents whose index is kept since they left.
  pub fn away_hosts(&self) -> Vec<String> {
    lock(&self.away).keys().cloned().collect()
  }

  /// Deletes the index kept for the agent `name`; false when none was.
  pub fn forget_away(&self, name: &str) -> bool {
    let Some(away) = lock(&self.away).remove(name) else {
      return false;
    };
    info!(
      agent = name,
      "kept share index of an agent no longer configured deleted"
    );
    away.discard();
    true
  }

  /// The agents that left whose kept index has the file a peer asked for by its exact remote
  /// name `masked`, leaving out an index that cannot be read; none when `view` hides it.
  pub async fn away_holding(&self, masked: &str, view: &View) -> Vec<String> {
    if view.hides(masked) {
      return Vec::new();
    }
    let kept: Vec<(String, Arc<Index>)> = lock(&self.away)
      .iter()
      .map(|(name, away)| (name.clone(), away.index.clone()))
      .collect();
    let mut holding = Vec::new();
    for (name, index) in kept {
      let wanted = masked.to_string();
      match index
        .read(move |conn| repository::find_file(conn, &wanted))
        .await
      {
        Ok(Some(_)) => holding.push(name),
        Ok(None) => {}
        Err(e) => {
          // an agent's index never fails the lookup
          drop(index.left_out(e));
        }
      }
    }
    holding
  }

  /// The agents' shares by host, in the order they were loaded, each with the visibility the
  /// share options in effect give its alias.
  pub fn agent_shares(&self) -> Vec<(String, Vec<ApiShare>)> {
    let options = lock(&self.applied).clone();
    lock(&self.hosts)
      .iter()
      .map(|h| {
        let shares = h
          .shares
          .iter()
          .map(|share| ApiShare {
            visible_to: visibility::of(&options, &share.alias),
            ..share.clone()
          })
          .collect();
        (h.name.clone(), shares)
      })
      .collect()
  }

  /// The local index then each agent's, in the order the agents were loaded.
  pub(super) fn databases(&self) -> Vec<Arc<Index>> {
    std::iter::once(Index::local(self.db.clone()))
      .chain(lock(&self.hosts).iter().map(|h| h.index.clone()))
      .collect()
  }

  /// The agents' indexes with their names, in the order they were loaded.
  pub(super) fn agent_databases(&self) -> Vec<(String, Arc<Index>)> {
    lock(&self.hosts)
      .iter()
      .map(|h| (h.name.clone(), h.index.clone()))
      .collect()
  }

  /// The host names and totals of the agents.
  pub(super) fn agent_totals(&self) -> (Vec<String>, SharedCounts) {
    let hosts = lock(&self.hosts);
    let names = hosts.iter().map(|h| h.name.clone()).collect();
    let sum = hosts
      .iter()
      .fold(SharedCounts::default(), |sum, h| SharedCounts {
        directories: sum.directories.saturating_add(h.directories),
        files: sum.files.saturating_add(h.files),
      });
    (names, sum)
  }

  /// Whether a local share or an agent's has the alias `lower`, given in lower case.
  pub(super) fn has_alias(&self, lower: &str) -> bool {
    let named = |alias: &str| alias.to_lowercase() == lower;
    lock(&self.shares).iter().any(|share| named(&share.alias))
      || lock(&self.hosts)
        .iter()
        .any(|h| h.shares.iter().any(|share| named(&share.alias)))
  }

  /// The totals over the agents' shares `view` sees, as each agent counted them.
  pub(super) fn agent_counts(&self, view: &View) -> SharedCounts {
    lock(&self.hosts)
      .iter()
      .flat_map(|h| &h.shares)
      .filter(|share| !view.hides_share(&share.alias))
      .fold(SharedCounts::default(), |sum, share| {
        sum.add(SharedCounts {
          directories: share.directories.unwrap_or(0),
          files: share.files.unwrap_or(0),
        })
      })
  }

  /// After an agent's host came or went: the state, the server's counts and the check of the
  /// visibility lists follow at once, the browse caches once the hosts have been still for a
  /// moment. A browse meanwhile finds its view's cache behind the change and writes it itself.
  fn hosts_changed(self: &Arc<Self>) {
    // numbered before the state tells of it, so a browse made once the state shows the change
    // finds every browse cache read before it behind
    let at = self.next_in_order();
    self.hosts_changed_at.fetch_max(at, Ordering::AcqRel);
    self.publish();
    self.check_unmatched();
    self.counts.send_replace(self.server_counts());
    if self.hosts_settling.swap(true, Ordering::AcqRel) {
      return;
    }
    let shares = self.clone();
    tokio::spawn(async move {
      tokio::select! {
        () = shares.shutdown.cancelled() => return,
        () = tokio::time::sleep(HOST_SETTLE) => {}
      }
      // a change from here on is a rebuild of its own
      shares.hosts_settling.store(false, Ordering::Release);
      shares.refresh_browse_caches().await;
    });
  }
}

/// The indexes of agents an earlier run kept in `cache`, each the newest file of its agent, by
/// agent, opened from disk for lookups whatever the storage mode, none when `keep` is false;
/// every other agent file there is deleted: older and unreadable indexes, journals and uploads
/// left behind. The files written from now on are numbered past the kept ones.
pub(super) async fn kept_agent_hosts(cache: &Path, keep: bool) -> HashMap<String, Away> {
  let mut newest: HashMap<String, (u64, PathBuf)> = HashMap::new();
  let mut unwanted = Vec::new();
  for (file, path) in agent_files(cache).await {
    let numbered = file
      .strip_prefix("shares.")
      .and_then(|rest| rest.strip_suffix(".db"))
      .and_then(|rest| rest.rsplit_once('.'))
      .and_then(|(name, number)| Some((name.to_string(), number.parse::<u64>().ok()?)));
    let Some((name, number)) = numbered.filter(|_| keep) else {
      // a kept index is read immutable, so no journal of an agent's file is ever wanted
      unwanted.push(path);
      continue;
    };
    HOST_FILES.fetch_max(number + 1, Ordering::Relaxed);
    match newest.get(&name) {
      Some((kept, _)) if *kept > number => unwanted.push(path),
      _ => {
        if let Some((_, older)) = newest.insert(name, (number, path)) {
          unwanted.push(older);
        }
      }
    }
  }
  for path in unwanted {
    info!(file = %path.display(), "agent share database of an earlier run deleted");
    // logs a file it could not remove
    if let Err(e) = tokio::task::spawn_blocking(move || remove_database(&path)).await {
      warn!(error = %e, "agent share database of an earlier run not deleted");
    }
  }
  let mut kept = HashMap::new();
  for (name, (_, path)) in newest {
    let opened = match storage::check_foreign(&path).await {
      Ok(()) => Database::open_immutable(&repository::SCHEMA, &path)
        .await
        .map_err(|e| e.to_string()),
      Err(e) => Err(e),
    };
    match opened {
      Ok(db) => {
        info!(agent = name, file = %path.display(), "kept share index of an agent found");
        let index = Arc::new(Index {
          db: Some(db),
          file: Mutex::new(None),
          agent: Some(name.clone()),
        });
        kept.insert(
          name,
          Away {
            index,
            file: Some(path),
          },
        );
      }
      Err(e) => {
        warn!(agent = name, error = %e, "kept share index of an agent not opened; deleted");
        delete(path);
      }
    }
  }
  kept
}

/// The agents' files in `cache`, by name and path: indexes, their journals and uploads.
async fn agent_files(cache: &Path) -> Vec<(String, PathBuf)> {
  let mut entries = match tokio::fs::read_dir(cache).await {
    Ok(entries) => entries,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
    Err(e) => {
      warn!(error = %e, "cache directory not read for agents' share databases");
      return Vec::new();
    }
  };
  let mut files = Vec::new();
  loop {
    let entry = match entries.next_entry().await {
      Ok(Some(entry)) => entry,
      Ok(None) => return files,
      Err(e) => {
        warn!(error = %e, "cache directory not read for agents' share databases");
        return files;
      }
    };
    let file = entry.file_name().to_string_lossy().into_owned();
    let Some(host) = host_of(&file) else {
      continue;
    };
    if host == LOCAL_HOST || host == format!("{LOCAL_HOST}.bak") {
      continue;
    }
    files.push((file, entry.path()));
  }
}

/// The host whose index `file` is, or one of its journal or temporary files, by name.
fn host_of(file: &str) -> Option<&str> {
  let rest = file.strip_prefix("shares.")?;
  let rest = rest
    .strip_suffix("-wal")
    .or_else(|| rest.strip_suffix("-shm"))
    .unwrap_or(rest);
  rest
    .strip_suffix(".db")
    .or_else(|| rest.strip_suffix(".db.tmp"))
    .filter(|host| !host.is_empty())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_host_is_read_from_its_files_names() {
    assert_eq!(host_of("shares.nas.db"), Some("nas"));
    assert_eq!(host_of("shares.nas.db-wal"), Some("nas"));
    assert_eq!(host_of("shares.my.nas.db.tmp"), Some("my.nas"));
    assert_eq!(host_of("shares.local.bak.db"), Some("local.bak"));
    assert_eq!(host_of("browse.cache"), None);
    assert_eq!(host_of("shares..db"), None);
  }

  #[tokio::test]
  async fn an_earlier_runs_agent_files_that_cannot_be_kept_are_deleted_and_the_local_ones_stay() {
    for keep in [true, false] {
      let dir = tempfile::tempdir().unwrap();
      let names = [
        "shares.local.db",
        "shares.local.db-wal",
        "shares.local.bak.db",
        "shares.local.bak.db.tmp",
        "shares.nas.db",
        "shares.nas.3.db",
        "shares.nas.3.db-wal",
        "shares.nas.4.db.tmp",
        "shares.nas.4.db.tmp-wal",
        "shares.a.b.7.db",
        "shares.gone.db",
        "shares.gone.db-shm",
        "shares.orphan.2.db-wal",
        "browse.cache",
      ];
      for name in names {
        std::fs::write(dir.path().join(name), "x").unwrap();
      }
      // none of them is an index, so the newest of each agent is not kept either
      let kept = kept_agent_hosts(dir.path(), keep).await;
      assert!(kept.is_empty());
      let left = || {
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
          .unwrap()
          .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
          .collect();
        left.sort();
        left
      };
      let local = [
        "browse.cache",
        "shares.local.bak.db",
        "shares.local.bak.db.tmp",
        "shares.local.db",
        "shares.local.db-wal",
      ];
      for _ in 0..200 {
        if left() == local {
          break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
      }
      assert_eq!(left(), local, "keeping: {keep}");
      if keep {
        assert!(
          HOST_FILES.load(Ordering::Relaxed) > 7,
          "new files are numbered past the kept"
        );
      }
    }
  }
}
