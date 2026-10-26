//! The state categories as SQL conditions on the `State` column of `Transfers`, so a query and
//! the listings agree on what "queued" or "failed" means.

use api_types::transfers::TransferStateCategory;

/// `Requested` and every `Queued` state.
pub const QUEUED: &str = "(State = 1 OR (State & 2 != 0 AND State & 16 = 0))";
/// `Initializing` and `InProgress`.
pub const IN_PROGRESS: &str = "(State IN (4, 8))";
/// Every `Completed` state.
pub const COMPLETED: &str = "(State & 16 != 0)";
/// `Completed, Succeeded`.
pub const SUCCESSFUL: &str = "(State = 48)";
/// Every `Completed` state other than `Completed, Succeeded`.
pub const FAILED: &str = "(State & 16 != 0 AND State != 48)";
/// `Completed` with `TimedOut`, `Errored`, `Rejected` or `Aborted`.
pub const ERRORED: &str = "(State & 16 != 0 AND State & 1920 != 0)";
/// `Completed, Cancelled`.
pub const CANCELLED: &str = "(State & 16 != 0 AND State & 64 != 0)";

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
      (16, &[C::Completed, C::Failed]),
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
}
