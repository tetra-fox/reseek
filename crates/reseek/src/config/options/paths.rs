//! Where reseek keeps its files and where downloads go.

use std::path::PathBuf;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

/// The four base directories. Each defaults to the platform's place for it, or to a directory
/// under the application directory when one is given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Paths {
  /// The configuration directory, which holds the configuration files and the scripts
  /// directory; set by the environment or the command line only, since the files are in it.
  #[cascade(env = "RESEEK_CONFIG_DIR", cli = "--config-dir")]
  pub config: Option<PathBuf>,
  /// The data directory, which holds the databases, runtime overrides and the instance lock.
  #[cascade(env = "RESEEK_DATA_DIR", cli = "--data-dir")]
  pub data: Option<PathBuf>,
  /// The cache directory, which holds what can be rebuilt, such as the share index.
  #[cascade(env = "RESEEK_CACHE_DIR", cli = "--cache-dir")]
  pub cache: Option<PathBuf>,
  /// The directory of the daily log files.
  #[cascade(env = "RESEEK_LOG_DIR", cli = "--log-dir")]
  pub logs: Option<PathBuf>,
}

/// Where downloads are written.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Directories {
  /// Where downloads in progress are written; `incomplete` in the data directory when not
  /// set. A directory given here must exist and be writable.
  #[cascade(env = "RESEEK_INCOMPLETE_DIR", cli = "--incomplete")]
  pub incomplete: Option<PathBuf>,
  /// Where finished downloads are moved; `downloads` in the data directory when not set. A
  /// directory given here must exist and be writable.
  #[cascade(env = "RESEEK_DOWNLOADS_DIR", cli = "-o|--downloads")]
  pub downloads: Option<PathBuf>,
}
