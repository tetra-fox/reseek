//! Where reseek keeps its files and where downloads go.

use std::path::PathBuf;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

/// The four base directories. Each defaults to the platform location for it, or to a directory
/// under the application directory when one is given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Paths {
  /// Holds the configuration files and scripts. Can only be set from the environment or the command
  /// line.
  #[cascade(env = "RESEEK_CONFIG_DIR", cli = "--config-dir")]
  pub config: Option<PathBuf>,
  /// Holds the databases and the settings applied at runtime.
  #[cascade(env = "RESEEK_DATA_DIR", cli = "--data-dir")]
  pub data: Option<PathBuf>,
  /// Holds data that can be rebuilt, such as the share index.
  #[cascade(env = "RESEEK_CACHE_DIR", cli = "--cache-dir")]
  pub cache: Option<PathBuf>,
  /// Holds the daily log files.
  #[cascade(env = "RESEEK_LOG_DIR", cli = "--log-dir")]
  pub logs: Option<PathBuf>,
}

/// Download directories.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Directories {
  /// Directory for downloads in progress. Defaults to `incomplete` in the data directory. A
  /// directory set here must exist and be writable.
  #[cascade(env = "RESEEK_INCOMPLETE_DIR", cli = "--incomplete")]
  pub incomplete: Option<PathBuf>,
  /// Directory for finished downloads. Defaults to `downloads` in the data directory. A
  /// directory set here must exist and be writable.
  #[cascade(env = "RESEEK_DOWNLOADS_DIR", cli = "-o|--downloads")]
  pub downloads: Option<PathBuf>,
}
