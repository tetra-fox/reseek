//! What the transfer history says about users: each user's figures, the counts the directory
//! sorts and filters by, a user's transfers page by page, and removing them from the listings.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use api_types::transfers::TransferDirection;
use api_types::users::{TransferFigures, UserFigures, UserTransfers, UserTransfersQuery};
use rusqlite::params_from_iter;
use rusqlite::types::Value;

use crate::db::{Database, DbError, read_timestamp};
use crate::sync::lock;
use crate::transfers::categories::{FAILED, IN_PROGRESS, QUEUED, SUCCESSFUL, condition};
use crate::transfers::{COLUMNS, NEWEST_FIRST, direction_text, transfer};

/// How long a user's figures are reused.
const FIGURES_FOR: Duration = Duration::from_secs(5);

/// A user's figures from the transfer history, the message counts left at zero.
pub async fn figures(transfers: &Database, username: &str) -> Result<UserFigures, DbError> {
  let username = username.to_string();
  transfers
    .read(move |conn| {
      let sql = format!(
        "SELECT Direction, \
           SUM(CASE WHEN {SUCCESSFUL} THEN 1 ELSE 0 END), \
           SUM(CASE WHEN {FAILED} THEN 1 ELSE 0 END), \
           SUM(CASE WHEN {IN_PROGRESS} AND Removed = 0 THEN 1 ELSE 0 END), \
           SUM(CASE WHEN {QUEUED} AND Removed = 0 THEN 1 ELSE 0 END), \
           SUM(CASE WHEN {SUCCESSFUL} THEN BytesTransferred ELSE 0 END), \
           MAX(RequestedAt) \
         FROM Transfers WHERE Username = ?1 GROUP BY Direction"
      );
      let mut statement = conn.prepare_cached(&sql)?;
      let mut rows = statement.query([&username])?;
      let mut figures = UserFigures {
        downloads: TransferFigures::default(),
        uploads: TransferFigures::default(),
        last_transfer_at: None,
        messages: Default::default(),
        last_message_at: None,
      };
      while let Some(row) = rows.next()? {
        let side = match crate::transfers::direction_at(row, 0)? {
          TransferDirection::Download => &mut figures.downloads,
          TransferDirection::Upload => &mut figures.uploads,
        };
        side.counts.succeeded = row.get(1)?;
        side.counts.failed = row.get(2)?;
        side.counts.active = row.get(3)?;
        side.counts.queued = row.get(4)?;
        side.bytes = row.get::<_, i64>(5)?.max(0).unsigned_abs();
        if let Some(text) = row.get::<_, Option<String>>(6)? {
          let at = read_timestamp(&text)?;
          figures.last_transfer_at = Some(figures.last_transfer_at.map_or(at, |t| t.max(at)));
        }
      }
      Ok(figures)
    })
    .await
}

/// Figures computed lately, by username, each kept for five seconds.
#[derive(Default)]
pub struct FigureCache {
  entries: Mutex<HashMap<String, (Instant, UserFigures)>>,
}

impl FigureCache {
  /// `username`'s figures from `transfers` and `messaging`, computed again when the kept ones
  /// are older than five seconds.
  pub async fn get(
    &self,
    transfers: &Database,
    messaging: &Database,
    username: &str,
  ) -> Result<UserFigures, DbError> {
    let kept = lock(&self.entries)
      .get(username)
      .filter(|(at, _)| at.elapsed() < FIGURES_FOR)
      .map(|(_, figures)| figures.clone());
    if let Some(figures) = kept {
      return Ok(figures);
    }
    let mut fresh = figures(transfers, username).await?;
    let name = username.to_string();
    (fresh.messages, fresh.last_message_at) = messaging
      .read(move |conn| crate::messaging::message_figures(conn, &name))
      .await?;
    let mut entries = lock(&self.entries);
    entries.retain(|_, (at, _)| at.elapsed() < FIGURES_FOR);
    entries.insert(username.to_string(), (Instant::now(), fresh.clone()));
    Ok(fresh)
  }

  /// Forgets `username`'s figures.
  pub fn forget(&self, username: &str) {
    lock(&self.entries).remove(username);
  }
}

/// The succeeded downloads and uploads of every user with a transfer, by username.
pub async fn succeeded_by_user(
  transfers: &Database,
) -> Result<HashMap<String, (u32, u32)>, DbError> {
  transfers
    .read(|conn| {
      let sql = format!(
        "SELECT Username, \
           SUM(CASE WHEN Direction = 'Download' AND {SUCCESSFUL} THEN 1 ELSE 0 END), \
           SUM(CASE WHEN Direction = 'Upload' AND {SUCCESSFUL} THEN 1 ELSE 0 END) \
         FROM Transfers WHERE Username IS NOT NULL GROUP BY Username"
      );
      let mut statement = conn.prepare_cached(&sql)?;
      let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?)))
      })?;
      rows.collect()
    })
    .await
}

/// A page of `username`'s transfers, newest first, removed ones included.
pub async fn page(
  transfers: &Database,
  username: &str,
  query: UserTransfersQuery,
) -> Result<UserTransfers, DbError> {
  let mut filter = String::from("Username = ?1");
  let mut values = vec![Value::Text(username.to_string())];
  if let Some(direction) = query.direction {
    values.push(Value::Text(direction_text(direction).to_string()));
    filter.push_str(&format!(" AND Direction = ?{}", values.len()));
  }
  if let Some(category) = query.state {
    filter.push_str(&format!(" AND {}", condition(category)));
  }
  transfers
    .read(move |conn| {
      let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM Transfers WHERE {filter}"),
        params_from_iter(&values),
        |row| row.get(0),
      )?;
      let mut paged = values.clone();
      paged.push(Value::Integer(i64::from(query.limit)));
      paged.push(Value::Integer(i64::from(query.offset)));
      let sql = format!(
        "SELECT {COLUMNS} FROM Transfers WHERE {filter} ORDER BY {NEWEST_FIRST} \
         LIMIT ?{} OFFSET ?{}",
        paged.len() - 1,
        paged.len()
      );
      let mut statement = conn.prepare(&sql)?;
      let rows = statement.query_map(params_from_iter(&paged), transfer)?;
      Ok(UserTransfers {
        transfers: rows.collect::<rusqlite::Result<_>>()?,
        total: total.unsigned_abs(),
      })
    })
    .await
}

/// Takes every transfer with `username` out of the listings, keeping them for the reports.
pub async fn remove_all(transfers: &Database, username: &str) -> Result<usize, DbError> {
  let username = username.to_string();
  transfers
    .write(move |conn| {
      conn.execute(
        "UPDATE Transfers SET Removed = 1, RemovedAt = ?2 WHERE Username = ?1 AND Removed = 0",
        rusqlite::params![username, crate::db::timestamp(chrono::Utc::now())],
      )
    })
    .await
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::fixtures::{Row, insert, transfers};
  use api_types::transfers::{TransferDirection, TransferStateCategory};

  #[tokio::test]
  async fn figures_count_each_category_with_removed_rows_only_in_the_history() {
    let db = transfers().await;
    let download = |state| Row::new("alice", TransferDirection::Download, state);
    let mut succeeded = download(48);
    succeeded.bytes = 700;
    let mut removed = download(48);
    removed.bytes = 300;
    removed.removed = true;
    let mut gone_queue = download(4098);
    gone_queue.removed = true;
    let mut latest = Row::new("alice", TransferDirection::Upload, 272);
    latest.requested_at = "2026-03-01T00:00:00Z".parse().unwrap();
    insert(
      &db,
      vec![
        succeeded,
        removed,
        download(80),
        download(1040),
        download(8),
        download(2050),
        gone_queue,
        latest,
        Row::new("bob", TransferDirection::Download, 48),
      ],
    )
    .await;
    let alice = figures(&db, "alice").await.unwrap();
    let downloads = &alice.downloads;
    assert_eq!(downloads.counts.succeeded, 2);
    assert_eq!(downloads.counts.failed, 2);
    assert_eq!(downloads.counts.active, 1);
    assert_eq!(downloads.counts.queued, 1, "a removed queued one is gone");
    assert_eq!(downloads.bytes, 1000);
    assert_eq!(alice.uploads.counts.failed, 1);
    assert_eq!(
      alice.last_transfer_at,
      Some("2026-03-01T00:00:00Z".parse().unwrap())
    );
    let by_user = succeeded_by_user(&db).await.unwrap();
    assert_eq!(by_user["alice"], (2, 0));
    assert_eq!(by_user["bob"], (1, 0));

    let failed = page(
      &db,
      "alice",
      UserTransfersQuery {
        direction: Some(TransferDirection::Download),
        state: Some(TransferStateCategory::Failed),
        offset: 0,
        limit: 1,
      },
    )
    .await
    .unwrap();
    assert_eq!(failed.total, 2);
    assert_eq!(failed.transfers.len(), 1);
    assert_eq!(
      remove_all(&db, "alice").await.unwrap(),
      6,
      "the removed ones stay as they are"
    );
  }
}
