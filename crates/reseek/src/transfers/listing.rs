//! The transfer listings of one direction: grouped by user and remote directory, a flat page
//! filtered and sorted, and one transfer with the states it went through.

use api_types::Page;
use api_types::transfers::{
  DirectoryResponse, SortOrder, Transfer, TransferDirection, TransferListQuery, TransferSort,
  TransferStateFilter, UserResponse,
};
use rusqlite::params_from_iter;
use rusqlite::types::Value;
use uuid::Uuid;

use super::categories::condition;
use super::{COLUMNS, Record, direction_text, store, transfer};
use crate::db::{Database, DbError, timestamp};

/// The remote directory of a remote filename: everything before its last separator, which is
/// `/` when the filename holds one and `\` otherwise; empty for a filename with neither.
pub fn remote_directory(filename: &str) -> &str {
  let separator = if filename.contains('/') { '/' } else { '\\' };
  filename
    .rsplit_once(separator)
    .map_or("", |(directory, _)| directory)
}

/// The transfers of `direction`, of `username` alone when given, grouped by user in the order
/// each user first asked or was asked, then by remote directory; removed ones only with
/// `include_removed`.
pub async fn grouped(
  db: &Database,
  direction: TransferDirection,
  include_removed: bool,
  username: Option<String>,
) -> Result<Vec<UserResponse>, DbError> {
  let transfers = db
    .read(move |conn| {
      let sql = format!(
        "SELECT {COLUMNS} FROM Transfers WHERE Direction = ?1 AND (?2 OR Removed = 0) \
         AND (?3 IS NULL OR Username = ?3) ORDER BY RequestedAt, Id"
      );
      let mut statement = conn.prepare_cached(&sql)?;
      let rows = statement.query_map(
        rusqlite::params![direction_text(direction), include_removed, username],
        transfer,
      )?;
      rows.collect::<rusqlite::Result<Vec<Transfer>>>()
    })
    .await?;
  Ok(group(transfers))
}

/// `transfers` grouped by user, then remote directory, each in the order first seen.
fn group(transfers: Vec<Transfer>) -> Vec<UserResponse> {
  let mut users: Vec<UserResponse> = Vec::new();
  for transfer in transfers {
    let at = match users.iter().position(|u| u.username == transfer.username) {
      Some(at) => at,
      None => {
        users.push(UserResponse {
          username: transfer.username.clone(),
          directories: Vec::new(),
        });
        users.len() - 1
      }
    };
    let directories = &mut users[at].directories;
    let name = remote_directory(&transfer.filename);
    let directory = match directories.iter().position(|d| d.directory == name) {
      Some(at) => &mut directories[at],
      None => {
        directories.push(DirectoryResponse {
          directory: name.to_string(),
          file_count: 0,
          files: Vec::new(),
        });
        directories.last_mut().expect("just pushed")
      }
    };
    directory.file_count += 1;
    directory.files.push(transfer);
  }
  users
}

/// One page of the transfers of `direction` that `query` picks, in its order, with how many it
/// picks over every page.
pub async fn page(
  db: &Database,
  direction: TransferDirection,
  query: TransferListQuery,
) -> Result<Page<Transfer>, DbError> {
  db.read(move |conn| {
    let mut conditions = vec!["Direction = ?".to_string()];
    let mut values = vec![Value::Text(direction_text(direction).to_string())];
    if !query.include_removed {
      conditions.push("Removed = 0".to_string());
    }
    if !query.state.is_empty() {
      let states: Vec<String> = query
        .state
        .iter()
        .map(|filter| match filter {
          TransferStateFilter::Category(category) => condition(*category).to_string(),
          TransferStateFilter::State(state) => format!("(State = {})", state.bits()),
        })
        .collect();
      conditions.push(format!("({})", states.join(" OR ")));
    }
    if let Some(username) = query.username {
      conditions.push("Username = ?".to_string());
      values.push(Value::Text(username));
    }
    if let Some(batch) = query.batch_id {
      conditions.push("BatchId = ?".to_string());
      values.push(Value::Text(batch.to_string()));
    }
    if let Some(since) = query.since {
      conditions.push("RequestedAt >= ?".to_string());
      values.push(Value::Text(timestamp(since)));
    }
    let filter = conditions.join(" AND ");
    let total: i64 = conn.query_row(
      &format!("SELECT COUNT(*) FROM Transfers WHERE {filter}"),
      params_from_iter(&values),
      |row| row.get(0),
    )?;
    let column = match query.sort {
      TransferSort::RequestedAt => "RequestedAt",
      TransferSort::EndedAt => "EndedAt",
      TransferSort::Username => "Username",
      TransferSort::Filename => "Filename",
      TransferSort::Size => "Size",
    };
    let order = match query.order {
      SortOrder::Asc => "ASC",
      SortOrder::Desc => "DESC",
    };
    values.push(Value::Integer(i64::from(query.limit)));
    values.push(Value::Integer(
      i64::try_from(query.offset).unwrap_or(i64::MAX),
    ));
    let sql = format!(
      "SELECT {COLUMNS} FROM Transfers WHERE {filter} ORDER BY {column} {order}, RequestedAt, Id \
       LIMIT ? OFFSET ?"
    );
    let mut statement = conn.prepare(&sql)?;
    let items = statement
      .query_map(params_from_iter(&values), transfer)?
      .collect::<rusqlite::Result<_>>()?;
    Ok(Page {
      items,
      total: total.unsigned_abs(),
    })
  })
  .await
}

/// The transfer `id` of `direction` with `username`, removed or not, with the states it went
/// through when `include_history`.
pub async fn one(
  db: &Database,
  direction: TransferDirection,
  username: &str,
  id: Uuid,
  include_history: bool,
) -> Result<Option<Transfer>, DbError> {
  let username = username.to_string();
  db.read(move |conn| {
    let Some(record) = store::get(conn, id)? else {
      return Ok(None);
    };
    if record.direction != direction || record.username != username {
      return Ok(None);
    }
    let mut transfer = record.transfer();
    if include_history {
      transfer.state_history = Some(store::history(conn, id)?);
    }
    Ok(Some(transfer))
  })
  .await
}

/// The record `id` of `direction` with `username`, removed or not.
pub async fn record(
  db: &Database,
  direction: TransferDirection,
  username: &str,
  id: Uuid,
) -> Result<Option<Record>, DbError> {
  let username = username.to_string();
  let record = db.read(move |conn| store::get(conn, id)).await?;
  Ok(record.filter(|r| r.direction == direction && r.username == username))
}

#[cfg(test)]
mod tests {
  use api_types::transfers::{TransferStateCategory, TransferStates};

  use super::*;
  use crate::transfers::fixtures::{Row, insert, transfers};

  #[test]
  fn the_remote_directory_follows_the_separator_the_filename_uses() {
    assert_eq!(
      remote_directory("@@abcde\\Music\\Album\\01.flac"),
      "@@abcde\\Music\\Album"
    );
    assert_eq!(remote_directory("Music/Album/01.flac"), "Music/Album");
    // a forward slash anywhere makes it the separator
    assert_eq!(remote_directory("Music\\AC/DC\\01.flac"), "Music\\AC");
    assert_eq!(remote_directory("01.flac"), "");
  }

  #[tokio::test]
  async fn transfers_group_by_user_then_directory_in_the_order_first_asked() {
    let db = transfers().await;
    let at = |minute: u32, user: &str, file: &str, removed: bool| {
      let mut row = Row::new(user, TransferDirection::Download, 2 | 2048);
      row.filename = file.to_string();
      row.requested_at = format!("2026-01-01T00:{minute:02}:00Z").parse().unwrap();
      row.removed = removed;
      row
    };
    insert(
      &db,
      vec![
        at(3, "alice", "a\\one.mp3", false),
        at(1, "bob", "b\\one.mp3", false),
        at(2, "alice", "c\\two.mp3", false),
        at(4, "alice", "a\\three.mp3", false),
        at(5, "carol", "z.mp3", true),
        at(6, "bob", "b\\upload.mp3", false),
      ],
    )
    .await;
    let mut upload = Row::new("bob", TransferDirection::Upload, 8);
    upload.filename = "b\\upload.mp3".to_string();
    insert(&db, vec![upload]).await;
    let listed = grouped(&db, TransferDirection::Download, false, None)
      .await
      .unwrap();
    let shape: Vec<(&str, Vec<(&str, u32)>)> = listed
      .iter()
      .map(|user| {
        (
          user.username.as_str(),
          user
            .directories
            .iter()
            .map(|d| (d.directory.as_str(), d.file_count))
            .collect(),
        )
      })
      .collect();
    assert_eq!(
      shape,
      [("bob", vec![("b", 2)]), ("alice", vec![("c", 1), ("a", 2)]),]
    );
    let with_removed = grouped(&db, TransferDirection::Download, true, None)
      .await
      .unwrap();
    assert_eq!(with_removed.last().unwrap().username, "carol");
  }

  #[tokio::test]
  async fn a_page_filters_sorts_and_counts_every_match() {
    let db = transfers().await;
    let row = |user: &str, state: u32, size: i64| {
      let mut row = Row::new(user, TransferDirection::Download, state);
      row.size = size;
      row
    };
    insert(
      &db,
      vec![
        row("alice", 48, 3),
        row("alice", 272, 1),
        row("bob", 80, 2),
        row("bob", 4098, 9),
      ],
    )
    .await;
    let query = |state: Vec<TransferStateFilter>, offset: u64| TransferListQuery {
      state,
      username: None,
      batch_id: None,
      include_removed: false,
      since: None,
      sort: TransferSort::Size,
      order: SortOrder::Desc,
      offset,
      limit: 1,
    };
    let failed = vec![TransferStateFilter::Category(TransferStateCategory::Failed)];
    let first = page(&db, TransferDirection::Download, query(failed.clone(), 0))
      .await
      .unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.items[0].size, 2);
    let second = page(&db, TransferDirection::Download, query(failed, 1))
      .await
      .unwrap();
    assert_eq!(second.items[0].size, 1);
    let exact = vec![TransferStateFilter::State(
      TransferStates::Queued | TransferStates::Remotely,
    )];
    let queued = page(&db, TransferDirection::Download, query(exact, 0))
      .await
      .unwrap();
    assert_eq!((queued.total, queued.items[0].size), (1, 9));
  }
}
