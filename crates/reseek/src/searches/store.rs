//! `search.db`: one row per search we started, its responses kept as one compressed blob
//! written when the search finished. While it runs, the responses it takes are saved in batches
//! beside it, so a stop that does not end it keeps them.

use std::io::{Read, Write};

use api_types::search::{
  Search, SearchListQuery, SearchOutcome, SearchResponse, SearchScope, SearchScopeType, SearchState,
};
use api_types::transfers::SortOrder;
use chrono::{DateTime, Utc};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};
use tracing::warn;
use uuid::Uuid;

use crate::db::{self, Schema, enum_at, enum_text, read_timestamp, timestamp};

/// `search.db`: the `searches` table and the wishlist.
pub static SCHEMA: Schema = db::schema!("search", ["0001_initial", "0002_running_responses"]);

/// The columns [`search`] reads, in its order: every one but the responses.
const COLUMNS: &str = "id, search_text, token, scope_type, scope_subjects, state, started_at, \
                       ended_at, response_count, file_count, locked_file_count, response_bytes";

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

/// The bit of a completed search in the stored state.
const COMPLETED: u32 = 4;

/// A search state as stored: the completed bit with one bit for how the search ended, or one bit
/// for a state before that, as the column's comment lists them.
fn state_bits(state: SearchState) -> u32 {
  match state {
    SearchState::Requested => 1,
    SearchState::InProgress => 2,
    SearchState::Queued => 256,
    SearchState::Completed(outcome) => {
      COMPLETED
        | match outcome {
          SearchOutcome::Cancelled => 8,
          SearchOutcome::TimedOut => 16,
          SearchOutcome::ResponseLimitReached => 32,
          SearchOutcome::FileLimitReached => 64,
          SearchOutcome::Errored => 128,
        }
    }
  }
}

/// The state stored as `bits`, which [`state_bits`] wrote.
fn state_from_bits(bits: u32) -> Option<SearchState> {
  SearchState::ALL
    .into_iter()
    .find(|state| state_bits(*state) == bits)
}

/// The search in `row`, read with [`COLUMNS`], without its responses.
fn search(row: &Row) -> rusqlite::Result<Search> {
  let id: String = row.get(0)?;
  let scope_type: SearchScopeType =
    enum_at(row, 3).map_err(|e| conversion(3, format!("search {id}: {e}")))?;
  let subjects: String = row.get(4)?;
  let bits: u32 = row.get(5)?;
  let state = state_from_bits(bits)
    .ok_or_else(|| conversion(5, format!("search {id}: {bits} is not a search state")))?;
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
    started_at: read_timestamp(&row.get::<_, String>(6)?)?,
    ended_at: ended_at.as_deref().map(read_timestamp).transpose()?,
    response_count: count(row, 8)?,
    file_count: count(row, 9)?,
    locked_file_count: count(row, 10)?,
    response_bytes: row.get::<_, i64>(11)?.max(0).unsigned_abs(),
    responses: None,
  })
}

/// A row [`list`] could not read.
pub struct Unreadable {
  /// the row's id, when that much could be read
  pub id: Option<String>,
  /// why it cannot be read
  pub error: rusqlite::Error,
}

/// The search in `row`, or the row as one that cannot be read; the outer error is the query's.
fn readable(row: &Row) -> rusqlite::Result<Result<Search, Unreadable>> {
  Ok(search(row).map_err(|error| Unreadable {
    id: row.get(0).ok(),
    error,
  }))
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
      enum_text(search.scope.scope_type),
      subjects,
      state_bits(search.state),
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
      state_bits(search.state),
      search.ended_at.map(timestamp),
      search.response_count,
      search.file_count,
      search.locked_file_count,
    ],
  )?;
  Ok(updated == 1)
}

/// Writes the state and counts of the running `search` with `batch`, the compressed responses
/// it took after the `first` it saved before, when it took any; false when it is gone.
pub fn save(
  conn: &mut Connection,
  search: &Search,
  first: usize,
  batch: Option<&[u8]>,
) -> rusqlite::Result<bool> {
  let tx = conn.transaction()?;
  if !update(&tx, search)? {
    return Ok(false);
  }
  if let Some(batch) = batch {
    tx.execute(
      "INSERT INTO running_responses (search_id, first, responses) VALUES (?1, ?2, ?3)",
      params![
        search.id.to_string(),
        i64::try_from(first).unwrap_or(i64::MAX),
        batch
      ],
    )?;
  }
  tx.commit()?;
  Ok(true)
}

/// Writes how `search` ended with its compressed `responses` and drops the batches it saved
/// while it ran, in one transaction, so a search reads as completed only once its responses are
/// there; false when it is gone.
pub fn finish(conn: &mut Connection, search: &Search, responses: &[u8]) -> rusqlite::Result<bool> {
  let tx = conn.transaction()?;
  let updated = tx.execute(
    "UPDATE searches SET state = ?2, ended_at = ?3, response_count = ?4, file_count = ?5, \
     locked_file_count = ?6, response_bytes = ?7, responses = ?8 WHERE id = ?1",
    params![
      search.id.to_string(),
      state_bits(search.state),
      search.ended_at.map(timestamp),
      search.response_count,
      search.file_count,
      search.locked_file_count,
      i64::try_from(responses.len()).unwrap_or(i64::MAX),
      responses,
    ],
  )?;
  tx.execute(
    "DELETE FROM running_responses WHERE search_id = ?1",
    [search.id.to_string()],
  )?;
  tx.commit()?;
  Ok(updated == 1)
}

/// The search `id` without its responses.
pub fn get(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<Search>> {
  conn
    .prepare_cached(&format!("SELECT {COLUMNS} FROM searches WHERE id = ?1"))?
    .query_row([id.to_string()], search)
    .optional()
}

/// Whether the search `id` is kept, found without reading its row.
pub fn exists(conn: &Connection, id: Uuid) -> rusqlite::Result<bool> {
  conn
    .prepare_cached("SELECT 1 FROM searches WHERE id = ?1")?
    .exists([id.to_string()])
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

/// The searches `query` asks for, without their responses, by when they started, and beside
/// them the rows this version cannot read, so one such row does not keep the others from being
/// listed.
pub fn list(
  conn: &Connection,
  query: &SearchListQuery,
) -> rusqlite::Result<(Vec<Search>, Vec<Unreadable>)> {
  let order = match query.order {
    SortOrder::Asc => "ASC",
    SortOrder::Desc => "DESC",
  };
  // -1 is no limit to SQLite
  let limit = query.limit.map_or(-1, i64::from);
  let (mut listed, mut unreadable) = (Vec::new(), Vec::new());
  let mut statement = conn.prepare(&format!(
    "SELECT {COLUMNS} FROM searches ORDER BY started_at {order}, rowid {order} \
     LIMIT ?1 OFFSET ?2"
  ))?;
  for row in statement.query_map(params![limit, query.offset], readable)? {
    match row? {
      Ok(search) => listed.push(search),
      Err(row) => unreadable.push(row),
    }
  }
  Ok((listed, unreadable))
}

/// Deletes the search `id` with the batches it saved while it ran, and clears the wishlist's
/// link to it. `None` when there is none, else the search as it was, without its responses, or
/// why its row cannot be read: such a row is deleted all the same.
pub fn delete(
  conn: &mut Connection,
  id: Uuid,
) -> rusqlite::Result<Option<rusqlite::Result<Search>>> {
  let tx = conn.transaction()?;
  let found = tx
    .prepare_cached(&format!("SELECT {COLUMNS} FROM searches WHERE id = ?1"))?
    .query_row([id.to_string()], |row| Ok(search(row)))
    .optional()?;
  if found.is_some() {
    let id = id.to_string();
    tx.execute("DELETE FROM searches WHERE id = ?1", [&id])?;
    tx.execute("DELETE FROM running_responses WHERE search_id = ?1", [&id])?;
    tx.execute(
      "UPDATE wishlist SET last_search_id = NULL WHERE last_search_id = ?1",
      [&id],
    )?;
  }
  tx.commit()?;
  Ok(found)
}

/// Deletes every search that ended before `before`, clears the wishlist's links to them, and
/// returns the ones it could read as they were. A row it cannot read is deleted and logged.
pub fn prune(conn: &mut Connection, before: DateTime<Utc>) -> rusqlite::Result<Vec<Search>> {
  const ENDED: &str = "ended_at IS NOT NULL AND ended_at < ?1";
  let before = timestamp(before);
  let tx = conn.transaction()?;
  let mut pruned = Vec::new();
  {
    let mut statement = tx.prepare(&format!("SELECT {COLUMNS} FROM searches WHERE {ENDED}"))?;
    for row in statement.query_map([&before], readable)? {
      match row? {
        Ok(search) => pruned.push(search),
        Err(row) => warn!(
          id = row.id.as_deref(),
          error = %row.error,
          "search pruned whose row cannot be read"
        ),
      }
    }
  }
  tx.execute(
    &format!(
      "UPDATE wishlist SET last_search_id = NULL \
       WHERE last_search_id IN (SELECT id FROM searches WHERE {ENDED})"
    ),
    [&before],
  )?;
  tx.execute(&format!("DELETE FROM searches WHERE {ENDED}"), [&before])?;
  tx.commit()?;
  Ok(pruned)
}

/// Ends the searches a stop left running. Each is completed as timed out at `now` with the
/// responses it saved while it ran and its counts made from them; a batch that cannot be read is
/// logged and left out. Every saved batch goes, as no search runs yet. Returns how many searches
/// there were.
pub fn end_dangling(conn: &mut Connection, now: DateTime<Utc>) -> rusqlite::Result<usize> {
  let tx = conn.transaction()?;
  let dangling: Vec<String> = tx
    .prepare("SELECT id FROM searches WHERE ended_at IS NULL OR state & ?1 = 0")?
    .query_map([COMPLETED], |row| row.get(0))?
    .collect::<rusqlite::Result<_>>()?;
  for id in &dangling {
    let responses = saved(&tx, id)?;
    let total = |count: fn(&SearchResponse) -> u32| {
      responses
        .iter()
        .fold(0u32, |sum, response| sum.saturating_add(count(response)))
    };
    let blob = compressed(&responses)?;
    tx.execute(
      "UPDATE searches SET state = ?2, ended_at = ?3, response_count = ?4, file_count = ?5, \
       locked_file_count = ?6, response_bytes = ?7, responses = ?8 WHERE id = ?1",
      params![
        id,
        state_bits(SearchState::Completed(SearchOutcome::TimedOut)),
        timestamp(now),
        u32::try_from(responses.len()).unwrap_or(u32::MAX),
        total(|response| response.file_count),
        total(|response| response.locked_file_count),
        i64::try_from(blob.len()).unwrap_or(i64::MAX),
        blob,
      ],
    )?;
  }
  tx.execute("DELETE FROM running_responses", [])?;
  tx.commit()?;
  Ok(dangling.len())
}

/// The responses the search `id` saved while it ran, in the order it took them; a batch that
/// cannot be read is logged and left out.
fn saved(conn: &Connection, id: &str) -> rusqlite::Result<Vec<SearchResponse>> {
  let mut statement = conn.prepare_cached(
    "SELECT first, responses FROM running_responses WHERE search_id = ?1 ORDER BY first",
  )?;
  let batches = statement.query_map([id], |row| {
    Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
  })?;
  let mut responses = Vec::new();
  for batch in batches {
    let (first, blob) = batch?;
    match decompress(&blob) {
      Ok(batch) => responses.extend(batch),
      Err(error) => warn!(
        search = id,
        first,
        %error,
        "saved search responses cannot be read; the search ends without them"
      ),
    }
  }
  Ok(responses)
}

/// `responses` as stored: a JSON array, zlib-compressed.
pub fn compress(responses: &[SearchResponse]) -> std::io::Result<Vec<u8>> {
  let json = serde_json::to_vec(responses)?;
  let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
  encoder.write_all(&json)?;
  encoder.finish()
}

/// [`compress`] for a write that stores the result.
pub fn compressed(responses: &[SearchResponse]) -> rusqlite::Result<Vec<u8>> {
  compress(responses).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
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
      state: SearchState::Requested,
      started_at: at.parse().unwrap(),
      ended_at: None,
      response_count: 0,
      file_count: 0,
      locked_file_count: 0,
      response_bytes: 0,
      responses: None,
    }
  }

  #[test]
  fn search_states_are_stored_as_the_bits_existing_rows_hold() {
    use SearchOutcome::*;
    assert_eq!(COMPLETED, 4);
    let stored = [
      (SearchState::Requested, 1),
      (SearchState::InProgress, 2),
      (SearchState::Queued, 256),
      (SearchState::Completed(Cancelled), 4 | 8),
      (SearchState::Completed(TimedOut), 4 | 16),
      (SearchState::Completed(ResponseLimitReached), 4 | 32),
      (SearchState::Completed(FileLimitReached), 4 | 64),
      (SearchState::Completed(Errored), 4 | 128),
    ];
    for (state, bits) in stored {
      assert_eq!(state_bits(state), bits, "{state}");
      assert_eq!(state_from_bits(bits), Some(state), "{bits}");
    }
    assert_eq!(
      state_from_bits(COMPLETED),
      None,
      "a completed search has an outcome"
    );
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
    search.state = SearchState::Completed(SearchOutcome::ResponseLimitReached);
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
  async fn a_search_stored_with_a_scope_this_version_does_not_know_is_an_error_naming_it() {
    let db = db().await;
    let search = requested("2026-10-26T10:00:00Z");
    let id = search.id;
    db.write(move |conn| {
      insert(conn, &search)?;
      conn.execute("UPDATE searches SET scope_type = 'galaxy'", [])
    })
    .await
    .unwrap();
    let error = db.read(move |conn| get(conn, id)).await.unwrap_err();
    assert!(error.to_string().contains(&id.to_string()), "{error}");
  }

  #[tokio::test]
  async fn a_row_with_an_unknown_state_is_listed_apart_by_its_id_and_deleted() {
    let db = db().await;
    let search = requested("2026-10-26T10:00:00Z");
    let id = search.id;
    db.write(move |conn| {
      insert(conn, &search)?;
      conn.execute("UPDATE searches SET state = 4", [])
    })
    .await
    .unwrap();
    let (listed, unreadable) = db
      .read(|conn| list(conn, &SearchListQuery::default()))
      .await
      .unwrap();
    assert!(listed.is_empty());
    assert_eq!(
      unreadable
        .iter()
        .map(|row| row.id.clone())
        .collect::<Vec<_>>(),
      [Some(id.to_string())]
    );
    let deleted = db.write(move |conn| delete(conn, id)).await.unwrap();
    assert!(matches!(deleted, Some(Err(_))), "found, though not read");
    assert!(!db.read(move |conn| exists(conn, id)).await.unwrap());
  }

  #[tokio::test]
  async fn the_startup_ends_dangling_searches_and_retention_prunes_ended_ones() {
    let db = db().await;
    let running = requested("2026-10-26T10:00:00Z");
    let mut lost = requested("2026-10-26T10:01:00Z");
    lost.state = SearchState::InProgress;
    lost.ended_at = Some("2026-10-26T10:02:00Z".parse().unwrap());
    let mut done = requested("2026-10-26T09:00:00Z");
    done.state = SearchState::Completed(SearchOutcome::TimedOut);
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
    assert_eq!(ended.state, SearchState::Completed(SearchOutcome::TimedOut));
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
    let (all, _) = db
      .read(|conn| list(conn, &SearchListQuery::default()))
      .await
      .unwrap();
    assert_eq!(
      all.iter().map(|s| s.id).collect::<Vec<_>>(),
      [running.id, lost.id],
      "oldest first"
    );
  }

  fn response(username: &str, files: usize, locked: usize) -> SearchResponse {
    let file = |locked| api_types::search::File {
      code: 1,
      filename: format!("@@{username}\\a.flac"),
      size: 1,
      extension: "flac".into(),
      bit_rate: None,
      length: None,
      is_variable_bit_rate: None,
      sample_rate: None,
      bit_depth: None,
      is_locked: locked,
      suspect: None,
      attribute_count: 0,
      attributes: Vec::new(),
    };
    SearchResponse {
      username: username.into(),
      country: None,
      token: 7,
      has_free_upload_slot: true,
      upload_speed: 0,
      queue_length: 0,
      file_count: files as u32,
      files: (0..files).map(|_| file(false)).collect(),
      locked_file_count: locked as u32,
      locked_files: (0..locked).map(|_| file(true)).collect(),
    }
  }

  #[tokio::test]
  async fn a_search_a_stop_left_running_ends_with_the_responses_it_saved() {
    let db = db().await;
    let mut search = requested("2026-10-26T10:00:00Z");
    search.state = SearchState::InProgress;
    let id = search.id;
    db.write(move |conn| {
      insert(conn, &search)?;
      let first = compressed(&[response("alice", 2, 0)])?;
      save(conn, &search, 0, Some(&first))?;
      let second = compressed(&[response("bob", 1, 1)])?;
      save(conn, &search, 1, Some(&second))
    })
    .await
    .unwrap();

    let now: DateTime<Utc> = "2026-10-26T12:00:00Z".parse().unwrap();
    assert_eq!(
      db.write(move |conn| end_dangling(conn, now)).await.unwrap(),
      1
    );
    let ended = db.read(move |conn| get(conn, id)).await.unwrap().unwrap();
    assert_eq!(ended.state, SearchState::Completed(SearchOutcome::TimedOut));
    assert_eq!(
      (
        ended.response_count,
        ended.file_count,
        ended.locked_file_count
      ),
      (2, 3, 1)
    );
    let blob = db
      .read(move |conn| responses(conn, id))
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    assert_eq!(ended.response_bytes, blob.len() as u64);
    let users: Vec<_> = decompress(&blob)
      .unwrap()
      .into_iter()
      .map(|r| r.username)
      .collect();
    assert_eq!(users, ["alice", "bob"], "in the order they were taken");
    let batches: i64 = db
      .read(|conn| conn.query_row("SELECT count(*) FROM running_responses", [], |r| r.get(0)))
      .await
      .unwrap();
    assert_eq!(batches, 0);
  }

  #[tokio::test]
  async fn deleting_or_pruning_a_search_clears_the_wishlists_link_to_it() {
    let db = db().await;
    let mut ended = requested("2026-10-26T09:00:00Z");
    ended.state = SearchState::Completed(SearchOutcome::TimedOut);
    ended.ended_at = Some("2026-10-26T09:00:15Z".parse().unwrap());
    let kept = requested("2026-10-26T10:00:00Z");
    let (old, new) = (ended.id, kept.id);
    db.write(move |conn| {
      insert(conn, &ended)?;
      insert(conn, &kept)?;
      conn.execute(
        "INSERT INTO wishlist (id, search_text, added_at, last_search_id) VALUES \
         ('a', 'x', '2026-10-26T08:00:00Z', ?1), ('b', 'y', '2026-10-26T08:00:00Z', ?2)",
        [old.to_string(), new.to_string()],
      )
    })
    .await
    .unwrap();
    let links = || {
      db.read(|conn| {
        conn
          .prepare("SELECT last_search_id FROM wishlist ORDER BY id")?
          .query_map([], |row| row.get::<_, Option<String>>(0))?
          .collect::<rusqlite::Result<Vec<_>>>()
      })
    };

    let before: DateTime<Utc> = "2026-10-26T11:00:00Z".parse().unwrap();
    db.write(move |conn| prune(conn, before)).await.unwrap();
    assert_eq!(links().await.unwrap(), [None, Some(new.to_string())]);
    let deleted = db.write(move |conn| delete(conn, new)).await.unwrap();
    assert!(matches!(deleted, Some(Ok(_))));
    assert_eq!(links().await.unwrap(), [None, None]);
  }
}
