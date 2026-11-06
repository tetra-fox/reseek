//! How often the recurring work runs: the cleanup, the database vacuum and the release check.
//! The share rescan is `shares.cache.retention`.

use std::time::Duration;

use cascade::Cascade;
use reseek_shares::schedule::Schedule;
use serde::{Deserialize, Serialize};

/// How often the recurring work runs, each `off`, an interval such as `5m`, or whole days at a
/// time of day such as `7d at 04:00`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Schedules {
  /// How often the cleanup deletes what the retention options let go: finished transfers and
  /// searches, chat history, events, users, files in the download directories, ended sessions
  /// and ended jobs. Minimum: 1 minute.
  #[cascade(env = "RESEEK_SCHEDULES_CLEANUP")]
  pub cleanup: Schedule,
  /// How often every database is rebuilt without its free pages. A database is vacuumed once
  /// nothing else writes it.
  #[cascade(env = "RESEEK_SCHEDULES_VACUUM")]
  pub vacuum: Schedule,
  /// How often the release feed is read for a newer version. Minimum: 1 hour.
  #[cascade(env = "RESEEK_SCHEDULES_RELEASE_CHECK")]
  pub release_check: Schedule,
}

impl Default for Schedules {
  fn default() -> Self {
    Self {
      cleanup: Schedule::every(Duration::from_secs(300)),
      vacuum: Schedule::Off,
      release_check: Schedule::every(Duration::from_secs(24 * 3600)),
    }
  }
}
