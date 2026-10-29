//! The retention sweep: completed transfers past the age their outcome is kept for are removed
//! from the listings, uploads first, then downloads, and, when set, transfers removed long
//! enough ago are deleted for good with their state history.

use std::time::Duration;

use api_types::transfers::TransferDirection;
use chrono::Utc;
use tracing::{debug, info, warn};

use super::categories::{CANCELLED, ERRORED, FAILED, SUCCESSFUL};
use super::removal::{self, Removed};
use crate::config::{TransferRetention, TransfersRetention};
use crate::db::{Database, DbError, shifted, timestamp};
use crate::live::Live;

/// The time `age` before now, as the database stores times.
fn before_now(age: Duration) -> String {
  let age = chrono::Duration::from_std(age).unwrap_or(chrono::Duration::MAX);
  timestamp(shifted(Utc::now(), -age))
}

/// The outcomes of one direction with how long each is kept, in the order they are swept.
fn outcomes(retention: &TransferRetention) -> [(&'static str, Option<Duration>, &'static str); 4] {
  [
    ("succeeded", retention.succeeded, SUCCESSFUL),
    ("canceled", retention.canceled, CANCELLED),
    ("errored", retention.errored, ERRORED),
    ("failed", retention.failed, FAILED),
  ]
}

/// Removes what `retention` no longer keeps and deletes what was removed long enough ago,
/// pushing each removal. A failure is logged and the rest of the sweep goes on.
pub async fn prune(db: &Database, live: &Live, retention: &TransfersRetention) {
  let mut failed = false;
  for (direction, kept) in [
    (TransferDirection::Upload, &retention.upload),
    (TransferDirection::Download, &retention.download),
  ] {
    for (outcome, age, condition) in outcomes(kept) {
      let Some(age) = age else { continue };
      match expire(db, direction, condition, age).await {
        Ok(removed) => {
          if !removed.is_empty() {
            debug!(
              ?direction,
              outcome,
              removed = removed.len(),
              "transfers past retention removed"
            );
          }
          removal::push(live, direction, &removed);
        }
        Err(e) => {
          warn!(?direction, outcome, error = %e, "transfers past retention not removed");
          failed = true;
        }
      }
    }
  }
  if let Some(after) = retention.purge_removed_after {
    match purge(db, after).await {
      Ok(0) => {}
      Ok(deleted) => info!(deleted, "removed transfers deleted for good"),
      Err(e) => {
        warn!(error = %e, "removed transfers not deleted");
        failed = true;
      }
    }
  }
  if failed {
    warn!("Encountered one or more errors while pruning transfers");
  }
}

/// Removes the transfers of `direction` in `condition` that ended more than `age` ago.
async fn expire(
  db: &Database,
  direction: TransferDirection,
  condition: &'static str,
  age: Duration,
) -> Result<Vec<Removed>, DbError> {
  let cutoff = before_now(age);
  db.write(move |conn| {
    removal::remove_where(
      conn,
      direction,
      &format!("EndedAt IS NOT NULL AND EndedAt < ?1 AND {condition}"),
      &cutoff,
    )
  })
  .await
}

/// Deletes the transfers removed more than `after` ago with their state history; returns how
/// many were deleted.
async fn purge(db: &Database, after: Duration) -> Result<usize, DbError> {
  let cutoff = before_now(after);
  db.write(move |conn| {
    let tx = conn.transaction()?;
    let removed_long_ago = "Removed = 1 AND RemovedAt IS NOT NULL AND RemovedAt < ?1";
    tx.execute(
      &format!(
        "DELETE FROM transfers_state_history WHERE transfer_id IN \
         (SELECT Id FROM Transfers WHERE {removed_long_ago})"
      ),
      [&cutoff],
    )?;
    let deleted = tx.execute(
      &format!("DELETE FROM Transfers WHERE {removed_long_ago}"),
      [&cutoff],
    )?;
    tx.commit()?;
    Ok(deleted)
  })
  .await
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::fixtures::{Row, insert, transfers};
  use crate::transfers::store;
  use tokio_util::sync::CancellationToken;

  const HOUR: Duration = Duration::from_secs(3600);

  fn ended(user: &str, state: u32, hours_ago: i64) -> Row {
    let mut row = Row::new(user, TransferDirection::Download, state);
    row.ended_at = Some(Utc::now() - chrono::Duration::hours(hours_ago));
    row
  }

  async fn removed(db: &Database) -> Vec<String> {
    let mut names: Vec<String> = db
      .read(|conn| {
        conn
          .prepare("SELECT Username FROM Transfers WHERE Removed = 1")?
          .query_map([], |row| row.get(0))?
          .collect()
      })
      .await
      .unwrap();
    names.sort();
    names
  }

  #[tokio::test]
  async fn each_outcome_is_removed_past_its_own_age_and_running_ones_never() {
    let db = transfers().await;
    let live = Live::new(CancellationToken::new());
    insert(
      &db,
      vec![
        ended("old-success", 48, 3),
        ended("new-success", 48, 1),
        ended("old-rejected", 528, 3),
        ended("old-canceled", 80, 3),
        Row::new("running", TransferDirection::Download, 8),
      ],
    )
    .await;
    let retention = TransfersRetention {
      download: TransferRetention {
        succeeded: Some(2 * HOUR),
        errored: Some(2 * HOUR),
        ..TransferRetention::default()
      },
      ..TransfersRetention::default()
    };
    prune(&db, &live, &retention).await;
    assert_eq!(removed(&db).await, ["old-rejected", "old-success"]);
  }

  #[tokio::test]
  async fn removed_transfers_are_deleted_once_removed_long_enough_with_their_history() {
    let db = transfers().await;
    let live = Live::new(CancellationToken::new());
    let mut long_ago = ended("long-ago", 48, 3);
    long_ago.removed = true;
    let mut lately = ended("lately", 48, 3);
    lately.removed = true;
    let kept = ended("kept", 48, 3);
    let (gone, stays) = (long_ago.id, lately.id);
    insert(&db, vec![long_ago, lately, kept]).await;
    db.write(move |conn| {
      let three_hours_ago = timestamp(Utc::now() - chrono::Duration::hours(3));
      conn.execute(
        "UPDATE Transfers SET RemovedAt = ?2 WHERE Id = ?1",
        rusqlite::params![gone.to_string(), three_hours_ago],
      )?;
      conn.execute(
        "UPDATE Transfers SET RemovedAt = ?2 WHERE Id = ?1",
        rusqlite::params![stays.to_string(), timestamp(Utc::now())],
      )?;
      store::entered(
        conn,
        gone,
        api_types::transfers::TransferStates::InProgress,
        Utc::now(),
      )
    })
    .await
    .unwrap();
    let retention = TransfersRetention {
      purge_removed_after: Some(HOUR),
      ..TransfersRetention::default()
    };
    prune(&db, &live, &retention).await;
    let left: Vec<String> = db
      .read(|conn| {
        conn
          .prepare("SELECT Username FROM Transfers ORDER BY Username")?
          .query_map([], |row| row.get(0))?
          .collect()
      })
      .await
      .unwrap();
    assert_eq!(left, ["kept", "lately"]);
    let history = db
      .read(move |conn| store::history(conn, gone))
      .await
      .unwrap();
    assert!(history.is_empty());
  }
}
