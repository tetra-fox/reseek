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
  /// How often old data is deleted, as set in Retention. Minimum: 1 minute.
  #[cascade(env = "RESEEK_SCHEDULES_CLEANUP")]
  pub cleanup: Schedule,
  /// How often the databases are compacted to give back unused disk space.
  #[cascade(env = "RESEEK_SCHEDULES_VACUUM")]
  pub vacuum: Schedule,
  /// How often reseek checks for a newer release. Minimum: 1 hour.
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
