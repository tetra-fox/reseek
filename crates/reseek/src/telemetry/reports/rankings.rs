//! Rankings over the successful transfers: users by what we moved with them, and our shared
//! directories by how many users downloaded from them.

use std::collections::{HashMap, HashSet};

use api_types::reports::{
  DirectoriesQuery, DirectoryCount, LeaderboardEntry, LeaderboardQuery, LeaderboardSort,
};
use api_types::transfers::{SortOrder, TransferDirection};
use rusqlite::params_from_iter;
use rusqlite::types::Value;

use super::{Filter, Reports, SUMMARY, Window, summary_at};
use crate::db::DbError;
use crate::transfers::categories::SUCCESSFUL;
use crate::transfers::text_at;

impl Reports {
  /// Users ranked by their successful transfers in one direction, ties broken by username
  /// descending.
  pub async fn leaderboard(
    &self,
    window: Window,
    query: LeaderboardQuery,
  ) -> Result<Vec<LeaderboardEntry>, DbError> {
    let filter = Filter::default()
      .ended_between(window)
      .direction(Some(query.direction))
      .states(SUCCESSFUL);
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
              username: text_at(row, 0)?,
              summary,
            })
          })?
          .collect()
      })
      .await
  }

  /// Our shared directories by how many distinct users downloaded from them successfully,
  /// then by how many downloads; a directory is a filename without its last segment.
  pub async fn directories(
    &self,
    window: Window,
    query: DirectoriesQuery,
  ) -> Result<Vec<DirectoryCount>, DbError> {
    let filter = Filter::default()
      .ended_between(window)
      .direction(Some(TransferDirection::Upload))
      .states(SUCCESSFUL)
      .username(query.username.as_deref());
    let sql = format!(
      "SELECT Filename, Username FROM Transfers{}",
      filter.clause()
    );
    let rows: Vec<(String, String)> = self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(&sql)?;
        statement
          .query_map(params_from_iter(&filter.values), |row| {
            Ok((text_at(row, 0)?, text_at(row, 1)?))
          })?
          .collect()
      })
      .await?;
    let mut by_directory: HashMap<String, (u64, HashSet<String>)> = HashMap::new();
    for (filename, username) in rows {
      let entry = by_directory
        .entry(directory_of(&filename).to_string())
        .or_default();
      entry.0 += 1;
      entry.1.insert(username);
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
}
