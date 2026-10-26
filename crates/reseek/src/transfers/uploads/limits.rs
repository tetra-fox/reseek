//! What one user may queue and fetch from us: the limits of their group over the global ones,
//! leaf by leaf, and the user's figures they are held against, from one query over the user's
//! uploads. Windows are rolling: the last 24 hours and the last 7 days before now, by when an
//! upload started. A megabyte is 1 000 000 bytes. A refused request is told why in the words
//! other clients know.

use api_types::transfers::{TransferDirection, TransferStates};
use api_types::transfers::{UploadLimitUsage, UploadLimitWindow, UploadLimits};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};

use crate::config::{Limit, Limits, Options};
use crate::db::timestamp;
use crate::transfers::direction_text;
use crate::users::{DEFAULT_GROUP, LEECHERS};

/// What a request over the queued files limit is told; clients ask again later.
pub const TOO_MANY_FILES: &str = "Too many files";
/// What a request over the queued megabytes limit is told; clients ask again later.
pub const TOO_MANY_MEGABYTES: &str = "Too many megabytes";

/// Bytes in a megabyte of the limits.
const MEGABYTE: u64 = 1_000_000;

/// A user's upload figures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
  /// uploads queued or in progress
  pub queued_files: u64,
  /// their full sizes
  pub queued_bytes: u64,
  /// uploads started in the last 7 days that failed
  pub weekly_failed_files: u64,
  /// uploads started in the last 7 days that succeeded
  pub weekly_succeeded_files: u64,
  /// their sizes
  pub weekly_succeeded_bytes: u64,
  /// uploads started in the last 24 hours that failed
  pub daily_failed_files: u64,
  /// uploads started in the last 24 hours that succeeded
  pub daily_succeeded_files: u64,
  /// their sizes
  pub daily_succeeded_bytes: u64,
}

/// The figures of `username` at `now`, removed uploads counted too.
pub fn statistics(
  conn: &Connection,
  username: &str,
  now: DateTime<Utc>,
) -> rusqlite::Result<Statistics> {
  let week = timestamp(now - Duration::days(7));
  let day = timestamp(now - Duration::days(1));
  let succeeded = (TransferStates::Completed | TransferStates::Succeeded).bits();
  conn.query_row(
    "SELECT \
       COUNT(*) FILTER (WHERE EndedAt IS NULL), \
       COALESCE(SUM(Size) FILTER (WHERE EndedAt IS NULL), 0), \
       COUNT(*) FILTER (WHERE StartedAt >= ?3 AND State IN (80, 144, 272, 528, 1040)), \
       COUNT(*) FILTER (WHERE StartedAt >= ?3 AND State = ?5), \
       COALESCE(SUM(Size) FILTER (WHERE StartedAt >= ?3 AND State = ?5), 0), \
       COUNT(*) FILTER (WHERE StartedAt >= ?4 AND State IN (80, 144, 272, 528, 1040)), \
       COUNT(*) FILTER (WHERE StartedAt >= ?4 AND State = ?5), \
       COALESCE(SUM(Size) FILTER (WHERE StartedAt >= ?4 AND State = ?5), 0) \
     FROM Transfers WHERE Username = ?1 AND Direction = ?2 \
       AND (EndedAt IS NULL OR StartedAt >= ?3)",
    params![
      username,
      direction_text(TransferDirection::Upload),
      week,
      day,
      succeeded
    ],
    |row| {
      let n =
        |i: usize| -> rusqlite::Result<u64> { Ok(row.get::<_, i64>(i)?.max(0).unsigned_abs()) };
      Ok(Statistics {
        queued_files: n(0)?,
        queued_bytes: n(1)?,
        weekly_failed_files: n(2)?,
        weekly_succeeded_files: n(3)?,
        weekly_succeeded_bytes: n(4)?,
        daily_failed_files: n(5)?,
        daily_succeeded_files: n(6)?,
        daily_succeeded_bytes: n(7)?,
      })
    },
  )
}

/// One window's limits after the fall-back to the global ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Window {
  files: Option<u32>,
  megabytes: Option<u32>,
  failures: Option<u32>,
}

impl Window {
  /// `group`'s limits, each one it leaves unset taken from `global`.
  fn of(group: &Limit, global: &Limit) -> Self {
    Self {
      files: group.files.or(global.files),
      megabytes: group.megabytes.or(global.megabytes),
      failures: group.failures.or(global.failures),
    }
  }

  /// Whether the window limits anything.
  fn configured(&self) -> bool {
    self.files.is_some() || self.megabytes.is_some() || self.failures.is_some()
  }

  /// Whether one more file over `files` already counted passes the files limit.
  fn over_files(&self, files: u64) -> bool {
    self.files.is_some_and(|limit| files + 1 > u64::from(limit))
  }

  /// Whether `size` more bytes over `bytes` already counted pass the megabytes limit.
  fn over_megabytes(&self, bytes: u64, size: u64) -> bool {
    self
      .megabytes
      .is_some_and(|limit| bytes.saturating_add(size) > u64::from(limit) * MEGABYTE)
  }

  /// Whether `failed` failures reach the failures limit.
  fn over_failures(&self, failed: u64) -> bool {
    self
      .failures
      .is_some_and(|limit| failed >= u64::from(limit))
  }
}

/// The limits that apply to a user of `group`: a user-defined group's own, the default group's
/// for the default group and any group the configuration does not have, the leechers' for the
/// leechers.
fn group_limits<'a>(options: &'a Options, group: &str) -> &'a Limits {
  let groups = &options.transfers.groups;
  match group {
    LEECHERS => &groups.leechers.upload.limits,
    DEFAULT_GROUP => &groups.default.upload.limits,
    name => groups
      .user_defined
      .get(name)
      .map_or(&groups.default.upload.limits, |defined| {
        &defined.upload.limits
      }),
  }
}

/// The queued, daily and weekly windows of `group`.
fn windows(options: &Options, group: &str) -> (Window, Window, Window) {
  let own = group_limits(options, group);
  let global = &options.transfers.upload.limits;
  (
    Window::of(&own.queued, &global.queued),
    Window::of(&own.daily, &global.daily),
    Window::of(&own.weekly, &global.weekly),
  )
}

/// Why one more upload of `size` bytes to a user of `group` with `figures` is refused, `None`
/// when it is not: the queued window first, then the last week, then the last day, each
/// checking files before megabytes. The queued window's answers are the ones clients ask
/// again after.
pub fn refusal(
  options: &Options,
  group: &str,
  figures: &Statistics,
  size: u64,
) -> Option<&'static str> {
  let (queued, daily, weekly) = windows(options, group);
  if queued.configured() {
    if queued.over_files(figures.queued_files) {
      return Some(TOO_MANY_FILES);
    }
    if queued.over_megabytes(figures.queued_bytes, size) {
      return Some(TOO_MANY_MEGABYTES);
    }
  }
  let periods = [
    (
      weekly,
      figures.weekly_failed_files,
      figures.weekly_succeeded_files,
      figures.weekly_succeeded_bytes,
      [
        "Too many failed transfers this week",
        "Too many files this week",
        "Too many megabytes this week",
      ],
    ),
    (
      daily,
      figures.daily_failed_files,
      figures.daily_succeeded_files,
      figures.daily_succeeded_bytes,
      [
        "Too many failed transfers today",
        "Too many files today",
        "Too many megabytes today",
      ],
    ),
  ];
  for (window, failed, files, bytes, [failures, too_many_files, too_many_megabytes]) in periods {
    if !window.configured() {
      continue;
    }
    if window.over_failures(failed) {
      return Some(failures);
    }
    if window.over_files(files) {
      return Some(too_many_files);
    }
    if window.over_megabytes(bytes, size) {
      return Some(too_many_megabytes);
    }
  }
  None
}

/// The figures of a user of `group` against the group's limits, per window.
pub fn usage(options: &Options, group: &str, figures: &Statistics) -> UploadLimits {
  let (queued, daily, weekly) = windows(options, group);
  let usage = |used: u64, limit: Option<u64>| UploadLimitUsage { used, limit };
  let bytes = |limit: Option<u32>| limit.map(|mb| u64::from(mb) * MEGABYTE);
  let period = |window: Window, failed: u64, files: u64, sent: u64| UploadLimitWindow {
    files: usage(files, window.files.map(u64::from)),
    bytes: usage(sent, bytes(window.megabytes)),
    failures: Some(usage(failed, window.failures.map(u64::from))),
  };
  UploadLimits {
    queued: UploadLimitWindow {
      files: usage(figures.queued_files, queued.files.map(u64::from)),
      bytes: usage(figures.queued_bytes, bytes(queued.megabytes)),
      failures: None,
    },
    daily: period(
      daily,
      figures.daily_failed_files,
      figures.daily_succeeded_files,
      figures.daily_succeeded_bytes,
    ),
    weekly: period(
      weekly,
      figures.weekly_failed_files,
      figures.weekly_succeeded_files,
      figures.weekly_succeeded_bytes,
    ),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::fixtures::{Row, insert, transfers};

  fn options(toml: &str) -> Options {
    toml::from_str(toml).unwrap()
  }

  #[test]
  fn every_refusal_in_its_order_with_files_before_megabytes() {
    let limits = "[transfers.upload.limits]\n\
                  queued = { files = 2, megabytes = 3 }\n\
                  weekly = { files = 10, megabytes = 30, failures = 4 }\n\
                  daily = { files = 5, megabytes = 20, failures = 2 }\n";
    let options = options(limits);
    let refused = |figures: Statistics, size: u64| refusal(&options, DEFAULT_GROUP, &figures, size);
    let none = Statistics::default();
    assert_eq!(refused(none, 1_000_000), None);
    let queued_files = Statistics {
      queued_files: 2,
      queued_bytes: 3_000_000,
      ..none
    };
    assert_eq!(
      refused(queued_files, 1),
      Some(TOO_MANY_FILES),
      "files first"
    );
    let queued_bytes = Statistics {
      queued_bytes: 2_500_000,
      ..none
    };
    assert_eq!(refused(queued_bytes, 500_001), Some(TOO_MANY_MEGABYTES));
    assert_eq!(refused(queued_bytes, 500_000), None, "at the limit passes");
    let cases = [
      (
        Statistics {
          weekly_failed_files: 4,
          weekly_succeeded_files: 10,
          ..none
        },
        "Too many failed transfers this week",
      ),
      (
        Statistics {
          weekly_succeeded_files: 10,
          weekly_succeeded_bytes: 30_000_000,
          ..none
        },
        "Too many files this week",
      ),
      (
        Statistics {
          weekly_succeeded_bytes: 30_000_000,
          ..none
        },
        "Too many megabytes this week",
      ),
      (
        Statistics {
          daily_failed_files: 2,
          ..none
        },
        "Too many failed transfers today",
      ),
      (
        Statistics {
          daily_succeeded_files: 5,
          ..none
        },
        "Too many files today",
      ),
      (
        Statistics {
          daily_succeeded_bytes: 20_000_000,
          ..none
        },
        "Too many megabytes today",
      ),
    ];
    for (figures, wanted) in cases {
      assert_eq!(refused(figures, 1), Some(wanted), "{figures:?}");
    }
  }

  #[test]
  fn a_group_takes_each_limit_it_leaves_unset_from_the_global_ones() {
    let options = options(
      "[transfers.upload.limits.queued]\nfiles = 1\nmegabytes = 1\n\
       [transfers.groups.user_defined.friends.upload.limits.queued]\nfiles = 100\n\
       [transfers.groups.leechers.upload.limits.daily]\nfailures = 1\n",
    );
    let one_queued = Statistics {
      queued_files: 1,
      ..Statistics::default()
    };
    assert_eq!(refusal(&options, "friends", &one_queued, 1), None);
    assert_eq!(
      refusal(&options, "friends", &one_queued, 1_000_001),
      Some(TOO_MANY_MEGABYTES),
      "the global megabytes"
    );
    assert_eq!(
      refusal(&options, "gone", &one_queued, 1),
      Some(TOO_MANY_FILES),
      "a group the configuration lacks takes the default group's"
    );
    let failed = Statistics {
      daily_failed_files: 1,
      ..Statistics::default()
    };
    assert_eq!(
      refusal(&options, LEECHERS, &failed, 1),
      Some("Too many failed transfers today"),
      "a window with only failures set in the group is configured"
    );
  }

  #[test]
  fn a_window_limiting_only_failures_globally_is_checked_for_a_group_without_its_own() {
    let options = options(
      "[transfers.upload.limits.daily]\nfailures = 1\n\
       [transfers.groups.user_defined.friends]\nmembers = [\"alice\"]\n",
    );
    let failed = Statistics {
      daily_failed_files: 1,
      ..Statistics::default()
    };
    assert_eq!(
      refusal(&options, "friends", &failed, 1),
      Some("Too many failed transfers today")
    );
    assert_eq!(
      refusal(&options, "friends", &Statistics::default(), 1),
      None
    );
  }

  #[tokio::test]
  async fn the_figures_count_by_when_an_upload_started_removed_ones_too() {
    let db = transfers().await;
    let now: DateTime<Utc> = "2026-05-10T12:00:00Z".parse().unwrap();
    let row = |state: u32, started_hours_ago: Option<i64>, size: i64| {
      let mut row = Row::new("alice", TransferDirection::Upload, state);
      row.size = size;
      row.started_at = started_hours_ago.map(|h| now - Duration::hours(h));
      row.ended_at = (state & 16 != 0).then_some(now);
      row
    };
    let mut removed = row(48, Some(30), 7);
    removed.removed = true;
    let mut download = Row::new("alice", TransferDirection::Download, 48);
    download.started_at = Some(now);
    download.ended_at = Some(now);
    insert(
      &db,
      vec![
        row(2 | 2048, None, 100),
        row(8, Some(1), 50),
        row(48, Some(2), 10),
        removed,
        row(80, Some(3), 1),
        row(528, Some(100), 1),
        row(48, Some(200), 1000),
        // failed before it started: counts nowhere
        row(1040, None, 1),
        download,
      ],
    )
    .await;
    let figures = db
      .read(move |conn| statistics(conn, "alice", now))
      .await
      .unwrap();
    assert_eq!(
      figures,
      Statistics {
        queued_files: 2,
        queued_bytes: 150,
        weekly_failed_files: 2,
        weekly_succeeded_files: 2,
        weekly_succeeded_bytes: 17,
        daily_failed_files: 1,
        daily_succeeded_files: 1,
        daily_succeeded_bytes: 10,
      }
    );
  }
}
