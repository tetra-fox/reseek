//! Forgetting users not seen for `retention.users`, unless we keep something about them: a
//! block, notes, tags, a group, being a buddy or being muted.

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::db::timestamp;

/// Deletes the users last seen before `before` that have nothing of ours, and returns them.
pub fn prune(conn: &Connection, before: DateTime<Utc>) -> rusqlite::Result<Vec<String>> {
  let mut statement = conn.prepare_cached(
    "DELETE FROM users WHERE last_seen_at < ?1 AND notes = '' AND tags = '[]' \
     AND group_override IS NULL AND buddy = 0 AND muted = 0 \
     AND username NOT IN (SELECT username FROM blocks) RETURNING username",
  )?;
  let rows = statement.query_map([timestamp(before)], |row| row.get(0))?;
  rows.collect()
}

#[cfg(test)]
mod tests {
  use super::super::blocking;
  use super::super::store::{Change, change, ensure};
  use super::super::testing::users_db;
  use super::*;
  use api_types::users::blocking::{Block, BlockScope, BlockSource};

  #[tokio::test]
  async fn only_users_with_nothing_of_ours_are_forgotten() {
    let db = users_db().await;
    let old: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();
    let mut pruned = db
      .write(move |conn| {
        for name in [
          "plain", "noted", "tagged", "grouped", "buddy", "muted", "blocked",
        ] {
          ensure(conn, name, old)?;
        }
        ensure(conn, "recent", old + chrono::Duration::days(60))?;
        change(conn, "noted", &Change::Notes("x".into()), old)?;
        change(conn, "tagged", &Change::Tags(vec!["t".into()]), old)?;
        change(conn, "grouped", &Change::Group(Some("friends".into())), old)?;
        change(conn, "buddy", &Change::Buddy(Some(Default::default())), old)?;
        change(conn, "muted", &Change::Muted(true), old)?;
        let block = Block {
          username: "blocked".into(),
          reason: String::new(),
          scopes: vec![BlockScope::Uploads],
          addresses: Vec::new(),
          created_at: Some(old),
          expires_at: None,
          created_by: Some("admin".into()),
          source: BlockSource::Api,
        };
        blocking::store(conn, &block, old)?;
        prune(conn, old + chrono::Duration::days(30))
      })
      .await
      .unwrap();
    pruned.sort();
    assert_eq!(pruned, ["plain"]);
  }
}
