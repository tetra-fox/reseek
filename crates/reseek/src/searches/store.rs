//! `search.db`: one row per search we started, its responses kept as one compressed blob
//! written when the search finished.

use std::io::{Read, Write};

use api_types::search::{
  Search, SearchListQuery, SearchResponse, SearchScope, SearchScopeType, SearchStates,
};
use api_types::transfers::SortOrder;
use chrono::{DateTime, Utc};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};
use uuid::Uuid;

use crate::db::{self, Schema, read_timestamp, timestamp};

/// `search.db`: the `searches` table and the wishlist.
pub static SCHEMA: Schema = db::schema!("search", ["0001_initial"]);

/// The columns [`search`] reads, in its order: every one but the responses.
const COLUMNS: &str = "id, search_text, token, scope_type, scope_subjects, state, started_at, \
                       ended_at, response_count, file_count, locked_file_count, response_bytes";

/// A scope type as stored.
fn scope_text(scope: SearchScopeType) -> &'static str {
  match scope {
    SearchScopeType::Network => "network",
    SearchScopeType::User => "user",
    SearchScopeType::Room => "room",
    SearchScopeType::Rooms => "rooms",
    SearchScopeType::Buddies => "buddies",
    SearchScopeType::Wishlist => "wishlist",
  }
}

fn conversion(
  i: usize,
  error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> rusqlite::Error {
  rusqlite::Error::FromSqlConversionFailure(i, Type::Text, error.into())
}

/// A count as stored; the counts are 32-bit on the API.
fn count(row: &Row, i: usize) -> rusqlite::Result<u32> {
  Ok(u32::try_from(row.get::<_, i64>(i)?.max(0)).unwrap_or(u32::MAX))
}

/// The search in `row`, read with [`COLUMNS`], without its responses.
fn search(row: &Row) -> rusqlite::Result<Search> {
  let id: String = row.get(0)?;
  let scope_type = match row.get_ref(3)?.as_str()? {
    "user" => SearchScopeType::User,
    "room" => SearchScopeType::Room,
    "rooms" => SearchScopeType::Rooms,
    "buddies" => SearchScopeType::Buddies,
    "wishlist" => SearchScopeType::Wishlist,
    _ => SearchScopeType::Network,
  };
  let subjects: String = row.get(4)?;
  let state = SearchStates::from_bits_retain(row.get::<_, u32>(5)?);
  let ended_at: Option<String> = row.get(7)?;
  Ok(Search {
    id: id.parse().map_err(|e| conversion(0, e))?,
    search_text: row.get(1)?,
    token: row.get(2)?,
    scope: SearchScope {
      scope_type,
      subjects: serde_json::from_str(&subjects).map_err(|e| conversion(4, e))?,
    },
    state,
    is_complete: state.contains(SearchStates::Completed),
    started_at: read_timestamp(&row.get::<_, String>(6)?)?,
    ended_at: ended_at.as_deref().map(read_timestamp).transpose()?,
    response_count: count(row, 8)?,
    file_count: count(row, 9)?,
    locked_file_count: count(row, 10)?,
    response_bytes: row.get::<_, i64>(11)?.max(0).unsigned_abs(),
    responses: None,
  })
}

/// Writes a new search; false when a search with its id exists already.
pub fn insert(conn: &Connection, search: &Search) -> rusqlite::Result<bool> {
  let subjects = serde_json::to_string(&search.scope.subjects)
    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
  let inserted = conn.execute(
    "INSERT INTO searches (id, search_text, token, scope_type, scope_subjects, state, \
     started_at, ended_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT (id) DO NOTHING",
    params![
      search.id.to_string(),
      search.search_text,
      search.token,
      scope_text(search.scope.scope_type),
      subjects,
      search.state.bits(),
      timestamp(search.started_at),
      search.ended_at.map(timestamp),
    ],
  )?;
  Ok(inserted == 1)
}

/// Writes the state, end and counts of `search`; false when it is gone.
pub fn update(conn: &Connection, search: &Search) -> rusqlite::Result<bool> {
  let updated = conn.execute(
    "UPDATE searches SET state = ?2, ended_at = ?3, response_count = ?4, file_count = ?5, \
     locked_file_count = ?6 WHERE id = ?1",
    params![
      search.id.to_string(),
      search.state.bits(),
      search.ended_at.map(timestamp),
      search.response_count,
      search.file_count,
      search.locked_file_count,
    ],
  )?;
  Ok(updated == 1)
}

/// Writes how `search` ended with its compressed `responses`, in one statement, so a search
/// reads as completed only once its responses are there; false when it is gone.
pub fn finish(conn: &Connection, search: &Search, responses: &[u8]) -> rusqlite::Result<bool> {
  let updated = conn.execute(
    "UPDATE searches SET state = ?2, ended_at = ?3, response_count = ?4, file_count = ?5, \
     locked_file_count = ?6, response_bytes = ?7, responses = ?8 WHERE id = ?1",
    params![
      search.id.to_string(),
      search.state.bits(),
      search.ended_at.map(timestamp),
      search.response_count,
      search.file_count,
      search.locked_file_count,
      i64::try_from(responses.len()).unwrap_or(i64::MAX),
      responses,
    ],
  )?;
  Ok(updated == 1)
}

/// The search `id` without its responses.
pub fn get(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<Search>> {
  conn
    .prepare_cached(&format!("SELECT {COLUMNS} FROM searches WHERE id = ?1"))?
    .query_row([id.to_string()], search)
    .optional()
}

/// The compressed responses of the search `id`: `None` without a search, `Some(None)` when
/// none are stored.
pub fn responses(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<Option<Vec<u8>>>> {
  conn
    .prepare_cached("SELECT responses FROM searches WHERE id = ?1")?
    .query_row([id.to_string()], |row| row.get(0))
    .optional()
}

/// The text of the search `id`, when there is one.
pub fn text(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<String>> {
  conn
    .prepare_cached("SELECT search_text FROM searches WHERE id = ?1")?
    .query_row([id.to_string()], |row| row.get(0))
    .optional()
}

/// The searches `query` asks for, without their responses, by when they started.
pub fn list(conn: &Connection, query: &SearchListQuery) -> rusqlite::Result<Vec<Search>> {
  let order = match query.order {
    SortOrder::Asc => "ASC",
    SortOrder::Desc => "DESC",
  };
  // -1 is no limit to SQLite
  let limit = query.limit.map_or(-1, i64::from);
  conn
    .prepare(&format!(
      "SELECT {COLUMNS} FROM searches ORDER BY started_at {order}, rowid {order} \
       LIMIT ?1 OFFSET ?2"
    ))?
    .query_map(params![limit, query.offset], search)?
    .collect()
}

/// Deletes the search `id` and returns it as it was, without its responses.
pub fn delete(conn: &mut Connection, id: Uuid) -> rusqlite::Result<Option<Search>> {
  let tx = conn.transaction()?;
  let found = get(&tx, id)?;
  if found.is_some() {
    tx.execute("DELETE FROM searches WHERE id = ?1", [id.to_string()])?;
  }
  tx.commit()?;
  Ok(found)
}

/// Deletes every search that ended before `before` and returns them as they were.
pub fn prune(conn: &mut Connection, before: DateTime<Utc>) -> rusqlite::Result<Vec<Search>> {
  let tx = conn.transaction()?;
  let pruned: Vec<Search> = tx
    .prepare(&format!(
      "SELECT {COLUMNS} FROM searches WHERE ended_at IS NOT NULL AND ended_at < ?1"
    ))?
    .query_map([timestamp(before)], search)?
    .collect::<rusqlite::Result<_>>()?;
  tx.execute(
    "DELETE FROM searches WHERE ended_at IS NOT NULL AND ended_at < ?1",
    [timestamp(before)],
  )?;
  tx.commit()?;
  Ok(pruned)
}

/// Ends the searches a stop left running: their responses were only ever in memory, so each
/// is completed as timed out at `now`, with no responses and its counts at zero. Returns how
/// many there were.
pub fn end_dangling(conn: &Connection, now: DateTime<Utc>) -> rusqlite::Result<usize> {
  conn.execute(
    "UPDATE searches SET state = ?1, ended_at = ?2, response_count = 0, file_count = 0, \
     locked_file_count = 0, response_bytes = 0, responses = NULL \
     WHERE ended_at IS NULL OR state & ?3 = 0",
    params![
      (SearchStates::Completed | SearchStates::TimedOut).bits(),
      timestamp(now),
      SearchStates::Completed.bits(),
    ],
  )
}

/// `responses` as stored: a JSON array, zlib-compressed.
pub fn compress(responses: &[SearchResponse]) -> std::io::Result<Vec<u8>> {
  let json = serde_json::to_vec(responses)?;
  let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
  encoder.write_all(&json)?;
  encoder.finish()
}

/// The responses [`compress`] stored.
pub fn decompress(stored: &[u8]) -> std::io::Result<Vec<SearchResponse>> {
  let mut json = Vec::new();
  ZlibDecoder::new(stored).read_to_end(&mut json)?;
  Ok(serde_json::from_slice(&json)?)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::Database;

  async fn db() -> Database {
    Database::memory(&SCHEMA).await
  }

  fn requested(at: &str) -> Search {
    Search {
      id: Uuid::new_v4(),
      search_text: "pink floyd -live".into(),
      token: 7,
      scope: SearchScope {
        scope_type: SearchScopeType::User,
        subjects: vec!["alice".into()],
      },
      state: SearchStates::Requested,
      is_complete: false,
      started_at: at.parse().unwrap(),
      ended_at: None,
      response_count: 0,
      file_count: 0,
      locked_file_count: 0,
      response_bytes: 0,
      responses: None,
    }
  }

  #[tokio::test]
  async fn a_search_reads_back_without_its_responses_until_it_finished() {
    let db = db().await;
    let mut search = requested("2026-10-26T10:00:00Z");
    let written = search.clone();
    assert!(db.write(move |conn| insert(conn, &written)).await.unwrap());
    let again = search.clone();
    assert!(
      !db.write(move |conn| insert(conn, &again)).await.unwrap(),
      "an id is taken once"
    );
    let id = search.id;
    assert_eq!(
      db.read(move |conn| responses(conn, id)).await.unwrap(),
      Some(None),
      "nothing is stored while it runs"
    );
    search.state = SearchStates::Completed | SearchStates::ResponseLimitReached;
    search.is_complete = true;
    search.ended_at = Some("2026-10-26T10:00:15Z".parse().unwrap());
    search.response_count = 1;
    let blob = compress(&[]).unwrap();
    let finished = search.clone();
    let stored = blob.clone();
    assert!(
      db.write(move |conn| finish(conn, &finished, &stored))
        .await
        .unwrap()
    );
    let read = db.read(move |conn| get(conn, id)).await.unwrap().unwrap();
    assert_eq!(read.response_bytes, blob.len() as u64);
    search.response_bytes = blob.len() as u64;
    assert_eq!(read, search);
    let blob = db
      .read(move |conn| responses(conn, id))
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    assert!(decompress(&blob).unwrap().is_empty());
  }

  #[tokio::test]
  async fn the_startup_ends_dangling_searches_and_retention_prunes_ended_ones() {
    let db = db().await;
    let running = requested("2026-10-26T10:00:00Z");
    let mut lost = requested("2026-10-26T10:01:00Z");
    lost.state = SearchStates::InProgress;
    lost.ended_at = Some("2026-10-26T10:02:00Z".parse().unwrap());
    let mut done = requested("2026-10-26T09:00:00Z");
    done.state = SearchStates::Completed | SearchStates::TimedOut;
    done.ended_at = Some("2026-10-26T09:00:15Z".parse().unwrap());
    for search in [running.clone(), lost.clone(), done.clone()] {
      db.write(move |conn| insert(conn, &search)).await.unwrap();
    }
    let blob = compress(&[]).unwrap();
    let finished = done.clone();
    db.write(move |conn| finish(conn, &finished, &blob))
      .await
      .unwrap();
    let now: DateTime<Utc> = "2026-10-26T12:00:00Z".parse().unwrap();
    assert_eq!(
      db.write(move |conn| end_dangling(conn, now)).await.unwrap(),
      2
    );
    let id = running.id;
    let ended = db.read(move |conn| get(conn, id)).await.unwrap().unwrap();
    assert_eq!(
      ended.state,
      SearchStates::Completed | SearchStates::TimedOut
    );
    assert_eq!(ended.ended_at, Some(now));
    let id = done.id;
    assert!(
      db.read(move |conn| responses(conn, id))
        .await
        .unwrap()
        .unwrap()
        .is_some(),
      "a finished search keeps its responses"
    );

    let before: DateTime<Utc> = "2026-10-26T11:00:00Z".parse().unwrap();
    let pruned = db.write(move |conn| prune(conn, before)).await.unwrap();
    assert_eq!(pruned.len(), 1);
    assert_eq!(pruned[0].id, done.id);
    let all = db
      .read(|conn| list(conn, &SearchListQuery::default()))
      .await
      .unwrap();
    assert_eq!(
      all.iter().map(|s| s.id).collect::<Vec<_>>(),
      [running.id, lost.id],
      "oldest first"
    );
  }
}
