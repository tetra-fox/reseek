//! What we share.

use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::StorageMode;

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
  /// Index of shared files.
  pub cache: ShareCache,
}

/// Index of shared files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
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
  /// Number of files read at once during a scan.
  #[cascade(
    env = "RESEEK_SHARE_CACHE_WORKERS",
    cli = "--share-cache-workers",
    apply = "restart"
  )]
  pub workers: u32,
  /// Interval between automatic rescans of the shares. Minimum: 1 hour. No automatic rescans
  /// when not set.
  #[cascade(env = "RESEEK_SHARE_CACHE_RETENTION", cli = "--share-cache-retention")]
  #[serde(with = "humantime_serde")]
  pub retention: Option<Duration>,
}

/// The most scan workers there may be.
pub const MAX_SCAN_WORKERS: u32 = 128;

impl Default for ShareCache {
  fn default() -> Self {
    let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
    Self {
      storage_mode: StorageMode::default(),
      workers: u32::try_from(processors).map_or(MAX_SCAN_WORKERS, |n| n.min(MAX_SCAN_WORKERS)),
      retention: None,
    }
  }
}
