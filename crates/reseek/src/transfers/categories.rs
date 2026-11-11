//! The state categories as SQL conditions on the `State` column of `Transfers`, so the
//! listings, the users directory and the reports agree on what "queued" or "failed" means.

use std::fmt;
use std::sync::LazyLock;

use api_types::transfers::{TransferOutcome, TransferStateCategory, TransferStates};

use super::state::in_states;

/// An SQL condition on the `State` column, built once from the states it holds.
pub struct Condition(LazyLock<String>);

impl Condition {
  const fn of(build: fn() -> String) -> Self {
    Self(LazyLock::new(build))
  }
}

impl std::ops::Deref for Condition {
  type Target = str;

  fn deref(&self) -> &str {
    &self.0
  }
}

impl fmt::Display for Condition {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self)
  }
}

/// the states `holds` holds
fn states(holds: fn(TransferStates) -> bool) -> String {
  in_states(
    TransferStates::ALL
      .into_iter()
      .filter(|state| holds(*state)),
  )
}

/// `Requested`, `Queued, Locally` and `Queued, Remotely`.
pub static QUEUED: Condition = Condition::of(|| {
  states(|state| {
    matches!(
      state,
      TransferStates::Requested | TransferStates::QueuedLocally | TransferStates::QueuedRemotely
    )
  })
});
/// `Initializing` and `InProgress`.
pub static IN_PROGRESS: Condition = Condition::of(|| {
  states(|state| {
    matches!(
      state,
      TransferStates::Initializing | TransferStates::InProgress
    )
  })
});
/// Every finished state.
pub static COMPLETED: Condition = Condition::of(|| states(TransferStates::is_completed));
/// `Completed, Succeeded`.
pub static SUCCESSFUL: Condition =
  Condition::of(|| states(|state| state == TransferStates::Completed(TransferOutcome::Succeeded)));
/// Every finished state but `Completed, Succeeded`.
pub static FAILED: Condition = Condition::of(|| {
  states(|state| {
    state.is_completed() && state != TransferStates::Completed(TransferOutcome::Succeeded)
  })
});
/// `Completed` with `TimedOut`, `Errored`, `Rejected` or `Aborted`.
pub static ERRORED: Condition = Condition::of(|| {
  states(|state| {
    matches!(
      state,
      TransferStates::Completed(
        TransferOutcome::TimedOut
          | TransferOutcome::Errored
          | TransferOutcome::Rejected
          | TransferOutcome::Aborted
      )
    )
  })
});
/// `Completed, Cancelled`.
pub static CANCELLED: Condition =
  Condition::of(|| states(|state| state == TransferStates::Completed(TransferOutcome::Cancelled)));
/// Not finished.
pub static UNFINISHED: Condition = Condition::of(|| states(|state| !state.is_completed()));

/// The condition a category stands for.
pub fn condition(category: TransferStateCategory) -> &'static Condition {
  match category {
    TransferStateCategory::Queued => &QUEUED,
    TransferStateCategory::InProgress => &IN_PROGRESS,
    TransferStateCategory::Completed => &COMPLETED,
    TransferStateCategory::Successful => &SUCCESSFUL,
    TransferStateCategory::Failed => &FAILED,
    TransferStateCategory::Errored => &ERRORED,
    TransferStateCategory::Cancelled => &CANCELLED,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::state::column;
  use rusqlite::Connection;

  #[test]
  fn each_state_falls_in_its_categories() {
    let conn = Connection::open_in_memory().unwrap();
    let matches = |state: TransferStates, category: TransferStateCategory| -> bool {
      conn
        .query_row(
          &format!("SELECT {} FROM (SELECT ?1 AS State)", condition(category)),
          [column(state)],
          |row| row.get(0),
        )
        .unwrap()
    };
    use TransferOutcome as O;
    use TransferStateCategory as C;
    use TransferStates as S;
    let cases: &[(TransferStates, &[C])] = &[
      (S::Requested, &[C::Queued]),
      (S::QueuedLocally, &[C::Queued]),
      (S::QueuedRemotely, &[C::Queued]),
      (S::Initializing, &[C::InProgress]),
      (S::InProgress, &[C::InProgress]),
      (S::Completed(O::Succeeded), &[C::Completed, C::Successful]),
      (
        S::Completed(O::Cancelled),
        &[C::Completed, C::Failed, C::Cancelled],
      ),
      (
        S::Completed(O::TimedOut),
        &[C::Completed, C::Failed, C::Errored],
      ),
      (
        S::Completed(O::Errored),
        &[C::Completed, C::Failed, C::Errored],
      ),
      (
        S::Completed(O::Rejected),
        &[C::Completed, C::Failed, C::Errored],
      ),
      (
        S::Completed(O::Aborted),
        &[C::Completed, C::Failed, C::Errored],
      ),
    ];
    assert_eq!(cases.len(), TransferStates::ALL.len());
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
