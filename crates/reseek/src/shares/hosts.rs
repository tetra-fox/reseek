//! The shares of relay agents. A controller loads the index each of its agents uploads as a
//! host of its own beside the local one; the queries peers are answered from then cover every
//! host, the local one first, and the totals and the browse cache count them all. A host is
//! loaded whole, from a file checked first, and goes the moment its agent does.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use api_types::shares::Share as ApiShare;
use tracing::{debug, info, warn};

use super::{LOCAL_HOST, SharedCounts, Shares, browse, repository, storage};
use crate::db::{Database, DbError, Location};
use crate::sync::lock;

/// How long the browse cache waits after an agent's host changed before it is rebuilt, so an
/// agent that comes and goes rebuilds it once.
const HOST_SETTLE: Duration = Duration::from_secs(2);

/// An agent's shares as loaded.
pub(super) struct Host {
  /// the agent's instance name
  pub name: String,
  /// the shares it listed, with its counts
  pub shares: Vec<ApiShare>,
  /// its index
  pub db: Database,
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
  /// it could not be copied or opened
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

/// The index file of the host `name` in the cache directory `cache`.
fn host_file(cache: &Path, name: &str) -> PathBuf {
  cache.join(format!("shares.{name}.db"))
}

impl Shares {
  /// The version of the share index's schema, which an agent's must equal.
  pub fn schema_version() -> u32 {
    storage::schema_version()
  }

  /// Writes a consistent copy of the local index to `path` and returns its size.
  pub async fn dump(&self, path: &Path) -> Result<u64, DbError> {
    storage::dump(&self.db, path).await
  }

  /// The local shares as the API shows them.
  pub fn local_shares(&self) -> Vec<ApiShare> {
    self
      .listed()
      .iter()
      .map(super::share::Share::to_api)
      .collect()
  }

  /// Whether a scan of the local shares is running.
  pub fn scanning(&self) -> bool {
    lock(&self.status).scanning
  }

  /// A number that grows each time the local index changed: a scan completed, the index was
  /// loaded, or watched changes were applied, these at most once a minute.
  pub fn changes(&self) -> tokio::sync::watch::Receiver<u64> {
    self.changed.subscribe()
  }

  /// Loads the index an agent uploaded to `upload` as the host `name` with its `shares`, in
  /// place of the one it had. The file is checked first; the host is replaced only once the new
  /// index is ready. Returns the totals the agent counted.
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
    let db = if self.in_memory {
      let scratch = std::env::temp_dir();
      let location = Location {
        data: scratch.clone(),
        backups: scratch,
        volatile: true,
        log_sql: false,
      };
      let db = Database::open_as(&repository::SCHEMA, &location, &format!("shares.{name}")).await?;
      storage::restore(&db, upload).await?;
      db
    } else {
      let target = host_file(&self.cache, name);
      let (from, to) = (upload.to_path_buf(), target.clone());
      tokio::task::spawn_blocking(move || storage::replace_host_file(&from, &to))
        .await
        .map_err(|e| HostError::Failed(e.to_string()))?
        .map_err(HostError::Failed)?;
      Database::open_immutable(&repository::SCHEMA, &target).await?
    };
    let counted = totals(&shares);
    let host = Host {
      name: name.to_string(),
      shares,
      db,
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
    self.hosts_changed();
    Ok(counted)
  }

  /// Drops the host `name`, whose agent left; false when it had none.
  pub fn remove_host(self: &Arc<Self>, name: &str) -> bool {
    let removed = {
      let mut hosts = lock(&self.hosts);
      let before = hosts.len();
      hosts.retain(|h| h.name != name);
      hosts.len() != before
    };
    if removed {
      debug!(agent = name, "agent's shares unloaded");
      self.hosts_changed();
    }
    removed
  }

  /// The agents' shares by host, in the order they were loaded.
  pub fn agent_shares(&self) -> Vec<(String, Vec<ApiShare>)> {
    lock(&self.hosts)
      .iter()
      .map(|h| (h.name.clone(), h.shares.clone()))
      .collect()
  }

  /// The local index then each agent's, in the order the agents were loaded.
  pub(super) fn databases(&self) -> Vec<Database> {
    std::iter::once(self.db.clone())
      .chain(lock(&self.hosts).iter().map(|h| h.db.clone()))
      .collect()
  }

  /// The agents' indexes with their names, in the order they were loaded.
  pub(super) fn agent_databases(&self) -> Vec<(String, Database)> {
    lock(&self.hosts)
      .iter()
      .map(|h| (h.name.clone(), h.db.clone()))
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

  /// `local` with the agents' totals added.
  pub(super) fn with_agents(&self, local: SharedCounts) -> SharedCounts {
    let (_, agents) = self.agent_totals();
    SharedCounts {
      directories: local.directories.saturating_add(agents.directories),
      files: local.files.saturating_add(agents.files),
    }
  }

  /// After an agent's host came or went: the state and the server's counts follow at once, the
  /// browse cache once the hosts have been still for a moment.
  fn hosts_changed(self: &Arc<Self>) {
    self.publish(|_| {});
    let local = *lock(&self.local_counts);
    self.counts.send_replace(self.with_agents(local));
    if self
      .hosts_settling
      .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
      return;
    }
    let shares = self.clone();
    tokio::spawn(async move {
      tokio::select! {
        () = shares.shutdown.cancelled() => return,
        () = tokio::time::sleep(HOST_SETTLE) => {}
      }
      shares
        .hosts_settling
        .store(false, std::sync::atomic::Ordering::Release);
      browse::refresh(&shares.databases(), &shares.browse_cache()).await;
    });
  }
}

/// Deletes the index files of hosts in `cache` that are neither the local one nor named in
/// `agents`, an earlier run's agents' that are never served again, each with its journal files.
pub async fn remove_unknown_hosts(cache: &Path, agents: Vec<String>) {
  let mut entries = match tokio::fs::read_dir(cache).await {
    Ok(entries) => entries,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
    Err(e) => return warn!(error = %e, "cache directory not read for agents' share databases"),
  };
  loop {
    let entry = match entries.next_entry().await {
      Ok(Some(entry)) => entry,
      Ok(None) => return,
      Err(e) => return warn!(error = %e, "cache directory not read for agents' share databases"),
    };
    let file = entry.file_name().to_string_lossy().into_owned();
    let Some(host) = host_of(&file) else {
      continue;
    };
    if host == LOCAL_HOST
      || host == format!("{LOCAL_HOST}.bak")
      || agents.iter().any(|a| *a == host)
    {
      continue;
    }
    match tokio::fs::remove_file(entry.path()).await {
      Ok(()) => info!(
        file,
        "share database of an agent no longer configured deleted"
      ),
      Err(e) => {
        warn!(file, error = %e, "share database of an agent no longer configured not deleted")
      }
    }
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
  async fn only_the_files_of_unknown_agents_are_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let names = [
      "shares.local.db",
      "shares.local.db-wal",
      "shares.local.bak.db",
      "shares.local.bak.db.tmp",
      "shares.nas.db",
      "shares.gone.db",
      "shares.gone.db-shm",
      "browse.cache",
    ];
    for name in names {
      std::fs::write(dir.path().join(name), "x").unwrap();
    }
    remove_unknown_hosts(dir.path(), vec!["nas".to_string()]).await;
    let mut left: Vec<String> = std::fs::read_dir(dir.path())
      .unwrap()
      .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
      .collect();
    left.sort();
    assert_eq!(
      left,
      [
        "browse.cache",
        "shares.local.bak.db",
        "shares.local.bak.db.tmp",
        "shares.local.db",
        "shares.local.db-wal",
        "shares.nas.db",
      ]
    );
  }
}
