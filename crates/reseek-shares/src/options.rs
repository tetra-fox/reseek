//! The share options: the `[shares]` section of the daemon's and the relay agent's
//! configuration.

use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

cascade::option_enum! {
  /// Where the share index is kept.
  pub enum StorageMode {
    /// in memory, restored at each start from the backup of the last scan, or rebuilt by a
    /// scan when there is no backup
    #[default]
    Memory = "memory",
    /// in a database file in the cache directory
    Disk = "disk",
  }
}

/// Shared directories.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Shares {
  /// Shared directories, each an absolute path. A leading `[alias]` sets an alias, and a
  /// leading `!` excludes the directory instead.
  #[cascade(env = "RESEEK_SHARED_DIR", cli = "-s|--shared")]
  pub directories: Vec<String>,
  /// Also shares the downloads directory.
  #[cascade(env = "RESEEK_SHARE_DOWNLOADS", cli = "--share-downloads")]
  pub share_downloads: bool,
  /// Regular expressions. Files whose full path matches any of them are not shared.
  #[cascade(env = "RESEEK_SHARE_FILTER", cli = "--share-filter")]
  pub filters: Vec<String>,
  /// Follows symbolic links to directories and files within the shares. Share roots are
  /// always opened through a link.
  pub follow_symlinks: bool,
  /// Aliases of the shares only buddies see, matched ignoring case; trusted buddies see them
  /// too. Everyone else is answered as if they were not shared. A buddy is recognized only when
  /// they connect from the address the server reports for them, so one on our own network sees
  /// what everyone sees. The server's list also covers a relay agent's shares by their alias;
  /// an agent's own list is not used.
  #[cascade(env = "RESEEK_SHARE_BUDDIES_ONLY")]
  pub buddies_only: Vec<String>,
  /// Aliases of the shares only trusted buddies see, matched ignoring case. Everyone else is
  /// answered as if they were not shared. A buddy is recognized only when they connect from the
  /// address the server reports for them, so one on our own network sees what everyone sees.
  /// The server's list also covers a relay agent's shares by their alias; an agent's own list
  /// is not used.
  #[cascade(env = "RESEEK_SHARE_TRUSTED_ONLY")]
  pub trusted_only: Vec<String>,
  /// Index of shared files.
  pub cache: ShareCache,
}

/// Index of shared files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ShareCache {
  /// Storage of the share index: `memory`, rebuilt at every start, or `disk`, kept in the
  /// cache directory.
  #[cascade(
    env = "RESEEK_SHARE_CACHE_STORAGE_MODE",
    cli = "--share-cache-storage-mode",
    apply = "restart"
  )]
  pub storage_mode: StorageMode,
  /// Number of files read at once during a scan. One per processor when not set.
  #[cascade(
    env = "RESEEK_SHARE_CACHE_WORKERS",
    cli = "--share-cache-workers",
    apply = "restart"
  )]
  pub workers: Option<u32>,
  /// Interval between automatic rescans of the shares. Minimum: 1 hour. No automatic rescans
  /// when not set.
  #[cascade(env = "RESEEK_SHARE_CACHE_RETENTION", cli = "--share-cache-retention")]
  #[serde(with = "cascade::duration")]
  pub retention: Option<Duration>,
}

/// The most scan workers there may be.
pub const MAX_SCAN_WORKERS: u32 = 128;

impl ShareCache {
  /// How many files a scan reads at once
  pub fn scan_workers(&self) -> u32 {
    self.workers.unwrap_or_else(|| {
      let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
      u32::try_from(processors).map_or(MAX_SCAN_WORKERS, |n| n.min(MAX_SCAN_WORKERS))
    })
  }
}
