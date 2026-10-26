//! The state categories as SQL conditions on the `State` column of `Transfers`, so the
//! listings, the users directory and the reports agree on what "queued" or "failed" means.

use api_types::transfers::TransferStateCategory;

/// `Requested`, bare `Queued`, `Queued, Locally` and `Queued, Remotely`.
pub const QUEUED: &str = "(State IN (1, 2, 2050, 4098))";
/// `Initializing` and `InProgress`.
pub const IN_PROGRESS: &str = "(State IN (4, 8))";
/// Every finished state: bare `Completed` and `Completed` with each outcome.
pub const COMPLETED: &str = "(State IN (16, 48, 80, 144, 272, 528, 1040))";
/// `Completed, Succeeded`.
pub const SUCCESSFUL: &str = "(State = 48)";
/// `Completed` with `Cancelled`, `TimedOut`, `Errored`, `Rejected` or `Aborted`; a bare
/// `Completed` is finished but neither succeeded nor failed.
pub const FAILED: &str = "(State IN (80, 144, 272, 528, 1040))";
/// `Completed` with `TimedOut`, `Errored`, `Rejected` or `Aborted`.
pub const ERRORED: &str = "(State IN (144, 272, 528, 1040))";
/// `Completed, Cancelled`.
pub const CANCELLED: &str = "(State = 80)";
/// Not finished: not ended, or in a state without `Completed`, so a row the daemon stopped
/// writing halfway counts as unfinished too.
pub const UNFINISHED: &str = "(EndedAt IS NULL OR (State & 16) = 0)";

/// The condition a category stands for.
pub fn condition(category: TransferStateCategory) -> &'static str {
  match category {
    TransferStateCategory::Queued => QUEUED,
    TransferStateCategory::InProgress => IN_PROGRESS,
    TransferStateCategory::Completed => COMPLETED,
    TransferStateCategory::Successful => SUCCESSFUL,
    TransferStateCategory::Failed => FAILED,
    TransferStateCategory::Errored => ERRORED,
    TransferStateCategory::Cancelled => CANCELLED,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use rusqlite::Connection;

  #[test]
  fn each_state_falls_in_its_categories() {
    let conn = Connection::open_in_memory().unwrap();
    let matches = |state: u32, category: TransferStateCategory| -> bool {
      conn
        .query_row(
          &format!("SELECT {} FROM (SELECT ?1 AS State)", condition(category)),
          [state],
          |row| row.get(0),
        )
        .unwrap()
    };
    use TransferStateCategory as C;
    let cases: &[(u32, &[C])] = &[
      (1, &[C::Queued]),
      (2050, &[C::Queued]),
      (4098, &[C::Queued]),
      (4, &[C::InProgress]),
      (8, &[C::InProgress]),
      (48, &[C::Completed, C::Successful]),
      (80, &[C::Completed, C::Failed, C::Cancelled]),
      (144, &[C::Completed, C::Failed, C::Errored]),
      (272, &[C::Completed, C::Failed, C::Errored]),
      (528, &[C::Completed, C::Failed, C::Errored]),
      (1040, &[C::Completed, C::Failed, C::Errored]),
      (2, &[C::Queued]),
      (16, &[C::Completed]),
    ];
    let all = [
      C::Queued,
      C::InProgress,
      C::Completed,
      C::Successful,
      C::Failed,
      C::Errored,
      C::Cancelled,
    ];
    for (state, wanted) in cases {
      for category in all {
        assert_eq!(
          matches(*state, category),
          wanted.contains(&category),
          "{state} {category:?}"
        );
      }
    }
  }

  #[test]
  fn a_row_is_unfinished_until_it_has_both_ended_and_completed() {
    let conn = Connection::open_in_memory().unwrap();
    let unfinished = |ended: Option<&str>, state: u32| -> bool {
      conn
        .query_row(
          &format!("SELECT {UNFINISHED} FROM (SELECT ?1 AS EndedAt, ?2 AS State)"),
          rusqlite::params![ended, state],
          |row| row.get(0),
        )
        .unwrap()
    };
    let at = Some("2026-01-01T00:00:00Z");
    assert!(unfinished(None, 8));
    assert!(unfinished(None, 48), "completed but never ended");
    assert!(unfinished(at, 4098), "ended but not completed");
    assert!(!unfinished(at, 48));
    assert!(!unfinished(at, 272));
  }
}
