//! Rankings over the successful transfers: users by what we moved with them, and our shared
//! directories by how many users downloaded from them.

use std::collections::{HashMap, HashSet};

use api_types::reports::{
  DirectoriesQuery, DirectoryCount, LeaderboardEntry, LeaderboardQuery, LeaderboardSort,
};
use api_types::transfers::{SortOrder, TransferDirection};
use rusqlite::params_from_iter;
use rusqlite::types::Value;

use super::{Filter, Reports, SUCCEEDED, SUMMARY, summary_at};
use crate::db::{self, DbError};

impl Reports {
  /// Users ranked by their successful transfers in one direction, ties broken by username
  /// descending.
  pub async fn leaderboard(
    &self,
    query: LeaderboardQuery,
  ) -> Result<Vec<LeaderboardEntry>, DbError> {
    let filter = Filter::default()
      .ended_between(
        query.start.unwrap_or_else(db::earliest),
        query.end.unwrap_or_else(db::latest),
      )
      .direction(Some(query.direction))
      .states(&SUCCEEDED.to_string());
    let column = match query.sort_by {
      LeaderboardSort::Count => "COUNT(*)",
      LeaderboardSort::TotalBytes => "SUM(Size)",
      LeaderboardSort::AverageSpeed => "AVG(AverageSpeed)",
    };
    let order = match query.sort_order {
      SortOrder::Asc => "ASC",
      SortOrder::Desc => "DESC",
    };
    let sql = format!(
      "SELECT Username, {SUMMARY} FROM Transfers{} GROUP BY Username \
       ORDER BY {column} {order}, Username DESC LIMIT ? OFFSET ?",
      filter.clause()
    );
    let mut values = filter.values;
    values.push(Value::Integer(query.limit.into()));
    values.push(Value::Integer(query.offset.into()));
    self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(&sql)?;
        statement
          .query_map(params_from_iter(&values), |row| {
            let mut summary = summary_at(row, 1)?;
            summary.distinct_users = 1;
            Ok(LeaderboardEntry {
              username: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
              summary,
            })
          })?
          .collect()
      })
      .await
  }

  /// Our shared directories by how many distinct users downloaded from them successfully,
  /// then by how many downloads; a directory is a filename without its last segment.
  pub async fn directories(&self, query: DirectoriesQuery) -> Result<Vec<DirectoryCount>, DbError> {
    let filter = Filter::default()
      .ended_between(
        query.start.unwrap_or_else(db::earliest),
        query.end.unwrap_or_else(db::latest),
      )
      .direction(Some(TransferDirection::Upload))
      .states(&SUCCEEDED.to_string())
      .username(query.username.as_deref());
    let sql = format!(
      "SELECT Filename, Username FROM Transfers{}",
      filter.clause()
    );
    let rows: Vec<(Option<String>, Option<String>)> = self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(&sql)?;
        statement
          .query_map(params_from_iter(&filter.values), |row| {
            Ok((row.get(0)?, row.get(1)?))
          })?
          .collect()
      })
      .await?;
    let mut by_directory: HashMap<String, (u64, HashSet<String>)> = HashMap::new();
    for (filename, username) in rows {
      let entry = by_directory
        .entry(directory_of(filename.as_deref().unwrap_or_default()).to_string())
        .or_default();
      entry.0 += 1;
      entry.1.insert(username.unwrap_or_default());
    }
    let mut counts: Vec<DirectoryCount> = by_directory
      .into_iter()
      .map(|(directory, (count, users))| DirectoryCount {
        directory,
        count,
        distinct_users: users.len() as u64,
      })
      .collect();
    counts.sort_by(|a, b| {
      b.distinct_users
        .cmp(&a.distinct_users)
        .then(b.count.cmp(&a.count))
        .then_with(|| a.directory.cmp(&b.directory))
    });
    Ok(
      counts
        .into_iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .collect(),
    )
  }
}

/// A filename without its last segment and the separators before it, `/` and `\` both
/// separators; empty for a name without one.
fn directory_of(filename: &str) -> &str {
  let separator = |c: char| c == '/' || c == '\\';
  filename
    .trim_end_matches(|c: char| !separator(c))
    .trim_end_matches(separator)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::telemetry::reports::tests::{at, row};
  use crate::transfers::fixtures::{insert, transfers};
  use api_types::transfers::TransferDirection::{Download, Upload};

  #[test]
  fn a_directory_is_the_name_without_its_last_segment() {
    assert_eq!(
      directory_of("@@share\\Artist\\Album\\01.mp3"),
      "@@share\\Artist\\Album"
    );
    assert_eq!(directory_of("a/b//c.flac"), "a/b");
    assert_eq!(directory_of("dir\\"), "dir");
    assert_eq!(directory_of("bare.mp3"), "");
  }

  fn leaderboard(sort_by: LeaderboardSort, sort_order: SortOrder) -> LeaderboardQuery {
    LeaderboardQuery {
      direction: Upload,
      start: None,
      end: None,
      sort_by,
      sort_order,
      limit: 25,
      offset: 0,
    }
  }

  #[tokio::test]
  async fn the_leaderboard_ranks_successful_transfers_and_breaks_ties_by_name() {
    let db = transfers().await;
    let sized = |user: &str, size, speed| {
      let mut r = row(user, Upload, 48, at(10, 0));
      r.size = size;
      r.speed = speed;
      r
    };
    insert(
      &db,
      vec![
        sized("alice", 100, 10.0),
        sized("alice", 100, 30.0),
        sized("bob", 500, 5.0),
        sized("carol", 50, 50.0),
        row("alice", Upload, 272, at(10, 0)),
        row("zed", Download, 48, at(10, 0)),
      ],
    )
    .await;
    let reports = Reports::new(db);
    let names = |entries: Vec<LeaderboardEntry>| {
      entries
        .into_iter()
        .map(|e| (e.username, e.summary.count))
        .collect::<Vec<_>>()
    };
    let by_count = reports
      .leaderboard(leaderboard(LeaderboardSort::Count, SortOrder::Desc))
      .await
      .unwrap();
    assert_eq!(by_count[0].summary.distinct_users, 1);
    assert_eq!(by_count[0].summary.average_speed, 20.0);
    assert_eq!(
      names(by_count),
      [("alice".into(), 2), ("carol".into(), 1), ("bob".into(), 1)],
      "a failed upload and a download do not count; ties by name descending"
    );
    let by_bytes = reports
      .leaderboard(leaderboard(LeaderboardSort::TotalBytes, SortOrder::Asc))
      .await
      .unwrap();
    assert_eq!(by_bytes[0].username, "carol");
    let mut paged = leaderboard(LeaderboardSort::AverageSpeed, SortOrder::Desc);
    paged.offset = 1;
    paged.limit = 1;
    assert_eq!(
      names(reports.leaderboard(paged).await.unwrap()),
      [("alice".into(), 2)]
    );
  }

  #[tokio::test]
  async fn directories_rank_by_distinct_downloaders_then_downloads() {
    let db = transfers().await;
    let upload = |user: &str, file: &str| {
      let mut r = row(user, Upload, 48, at(10, 0));
      r.filename = file.to_string();
      r
    };
    insert(
      &db,
      vec![
        upload("a", "@@s\\One\\1.mp3"),
        upload("a", "@@s\\One\\2.mp3"),
        upload("a", "@@s\\One\\3.mp3"),
        upload("a", "@@s\\Two\\1.mp3"),
        upload("b", "@@s\\Two\\2.mp3"),
        row("c", Download, 48, at(10, 0)),
      ],
    )
    .await;
    let counts = Reports::new(db)
      .directories(DirectoriesQuery {
        start: None,
        end: None,
        username: None,
        limit: 25,
        offset: 0,
      })
      .await
      .unwrap();
    let ranked: Vec<_> = counts
      .iter()
      .map(|c| (c.directory.as_str(), c.distinct_users, c.count))
      .collect();
    assert_eq!(ranked, [("@@s\\Two", 2, 2), ("@@s\\One", 1, 3)]);
  }
}
