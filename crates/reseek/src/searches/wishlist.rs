//! The wishlist: saved searches run again with the server's wishlist search, one item a turn,
//! the item searched longest ago first, on the interval the server gives at login. Each item
//! remembers the files it found by user and filename, so a turn that finds new ones tells of
//! them with a `search.wishlist_found` frame.

use std::time::Duration;

use api_types::jobs::JobEnding;
use api_types::live::LiveFrame;
use api_types::search::{SearchRequest, WishlistFound, WishlistItem};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Row, params};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::Searches;
use crate::api::error::ApiError;
use crate::db::{read_timestamp, timestamp};
use crate::jobs::RunOutcome;
use crate::services::SearchService;

/// How long a wishlist search waits for another response.
const WAIT: Duration = Duration::from_secs(15);

/// The most users a found frame names.
const NAMED: usize = 5;

/// The longest wishlist text kept.
const LONGEST: usize = 256;

fn item(row: &Row) -> rusqlite::Result<WishlistItem> {
  let id = |i: usize| -> rusqlite::Result<Option<Uuid>> {
    row
      .get::<_, Option<String>>(i)?
      .map(|text| {
        text.parse().map_err(|e: uuid::Error| {
          rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, e.into())
        })
      })
      .transpose()
  };
  Ok(WishlistItem {
    id: id(0)?.ok_or(rusqlite::Error::InvalidColumnType(
      0,
      "id".to_string(),
      rusqlite::types::Type::Null,
    ))?,
    search_text: row.get(1)?,
    added_at: read_timestamp(&row.get::<_, String>(2)?)?,
    last_searched_at: row
      .get::<_, Option<String>>(3)?
      .map(|text| read_timestamp(&text))
      .transpose()?,
    last_search_id: id(4)?,
    last_new_files: row.get(5)?,
  })
}

const ITEM_COLUMNS: &str =
  "id, search_text, added_at, last_searched_at, last_search_id, last_new_files";

fn list(conn: &Connection) -> rusqlite::Result<Vec<WishlistItem>> {
  conn
    .prepare(&format!(
      "SELECT {ITEM_COLUMNS} FROM wishlist ORDER BY added_at, id"
    ))?
    .query_map([], item)?
    .collect()
}

/// The item searched longest ago, one never searched first.
fn next(conn: &Connection) -> rusqlite::Result<Option<WishlistItem>> {
  conn
    .query_row(
      &format!(
        "SELECT {ITEM_COLUMNS} FROM wishlist \
         ORDER BY last_searched_at IS NOT NULL, last_searched_at, added_at, id LIMIT 1"
      ),
      [],
      item,
    )
    .optional()
}

/// Marks the files `found` as seen by the item `id` and records its turn; returns those not
/// seen before.
fn seen(
  conn: &mut Connection,
  id: Uuid,
  search: Uuid,
  found: &[(String, String)],
) -> rusqlite::Result<Vec<(String, String)>> {
  let tx = conn.transaction()?;
  let mut fresh = Vec::new();
  {
    let mut insert = tx.prepare_cached(
      "INSERT OR IGNORE INTO wishlist_seen (item_id, username, filename) VALUES (?1, ?2, ?3)",
    )?;
    for (username, filename) in found {
      if insert.execute(params![id.to_string(), username, filename])? > 0 {
        fresh.push((username.clone(), filename.clone()));
      }
    }
  }
  tx.execute(
    "UPDATE wishlist SET last_searched_at = ?2, last_search_id = ?3, last_new_files = ?4 \
     WHERE id = ?1",
    params![
      id.to_string(),
      timestamp(Utc::now()),
      search.to_string(),
      u32::try_from(fresh.len()).unwrap_or(u32::MAX)
    ],
  )?;
  tx.commit()?;
  Ok(fresh)
}

impl Searches {
  /// How often the server lets the wishlist search, while logged in and once it said.
  pub fn wishlist_interval(&self) -> Option<Duration> {
    let client = self.network.client();
    if !client
      .state()
      .contains(soulseek_client::ClientState::LOGGED_IN)
    {
      return None;
    }
    client
      .server_info()
      .wishlist_interval
      .filter(|seconds| *seconds > 0)
      .map(|seconds| Duration::from_secs(u64::from(seconds)))
  }

  /// Every wishlist item, oldest first.
  pub async fn wishlist(&self) -> Result<Vec<WishlistItem>, ApiError> {
    self.db.read(list).await.map_err(ApiError::internal)
  }

  /// Adds `text` to the wishlist, trimmed; an item already there with the same text, ignoring
  /// case, is answered instead.
  pub async fn add_wish(&self, text: &str) -> Result<WishlistItem, ApiError> {
    let text = text.trim().to_string();
    if text.is_empty() {
      return Err(ApiError::bad_request("The search text is empty"));
    }
    if text.chars().count() > LONGEST {
      return Err(ApiError::bad_request(format!(
        "The search text is longer than {LONGEST} characters"
      )));
    }
    let added = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let found = tx
          .query_row(
            &format!("SELECT {ITEM_COLUMNS} FROM wishlist WHERE lower(search_text) = lower(?1)"),
            [&text],
            item,
          )
          .optional()?;
        if let Some(found) = found {
          return Ok((found, false));
        }
        let id = Uuid::new_v4();
        tx.execute(
          "INSERT INTO wishlist (id, search_text, added_at) VALUES (?1, ?2, ?3)",
          params![id.to_string(), text, timestamp(Utc::now())],
        )?;
        let added = tx.query_row(
          &format!("SELECT {ITEM_COLUMNS} FROM wishlist WHERE id = ?1"),
          [id.to_string()],
          item,
        )?;
        tx.commit()?;
        Ok((added, true))
      })
      .await
      .map_err(ApiError::internal)?;
    let (item, new) = added;
    if new {
      info!(id = %item.id, text = item.search_text, "wishlist item added");
    }
    Ok(item)
  }

  /// Removes the wishlist item `id` and what it found; 404 when there is none.
  pub async fn remove_wish(&self, id: Uuid) -> Result<(), ApiError> {
    let removed = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        tx.execute(
          "DELETE FROM wishlist_seen WHERE item_id = ?1",
          [id.to_string()],
        )?;
        let removed = tx.execute("DELETE FROM wishlist WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
        Ok(removed > 0)
      })
      .await
      .map_err(ApiError::internal)?;
    if !removed {
      return Err(ApiError::not_found(format!("No wishlist item {id}")));
    }
    info!(%id, "wishlist item removed");
    Ok(())
  }

  /// One turn of the wishlist: the item searched longest ago is searched with the server's
  /// wishlist search, and the files it finds that it had not found before are told of.
  pub async fn wishlist_turn(&self) -> RunOutcome {
    let failed = |error: String| RunOutcome {
      ending: JobEnding::Failed,
      error: Some(error),
      summary: None,
      job_id: None,
    };
    let item = match self.db.read(next).await {
      Ok(Some(item)) => item,
      Ok(None) => {
        return RunOutcome {
          ending: JobEnding::Succeeded,
          error: None,
          summary: Some("The wishlist is empty".to_string()),
          job_id: None,
        };
      }
      Err(e) => return failed(e.to_string()),
    };
    let request: SearchRequest = match serde_json::from_value(serde_json::json!({
      "searchText": item.search_text,
      "scope": { "type": "wishlist" },
    })) {
      Ok(request) => request,
      Err(e) => return failed(e.to_string()),
    };
    let search = match self.start(request, WAIT).await {
      Ok(search) => search,
      Err(e) => {
        debug!(
          text = item.search_text,
          error = e.message(),
          "wishlist search not started"
        );
        return failed(e.message().to_string());
      }
    };
    let running = self.running().get(&search.id).cloned();
    if let Some(running) = running {
      let mut done = running.done.subscribe();
      if done.wait_for(|done| *done).await.is_err() {
        warn!(id = %search.id, "a wishlist search ended without saying");
      }
    }
    let responses = match self.responses(search.id).await {
      Ok(responses) => responses,
      Err(e) => return failed(e.message().to_string()),
    };
    let found: Vec<(String, String)> = responses
      .iter()
      .flat_map(|response| {
        response
          .files
          .iter()
          .map(|file| (response.username.clone(), file.filename.clone()))
      })
      .collect();
    let (id, search_id) = (item.id, search.id);
    let fresh = match self
      .db
      .write(move |conn| seen(conn, id, search_id, &found))
      .await
    {
      Ok(fresh) => fresh,
      Err(e) => return failed(e.to_string()),
    };
    let count = u32::try_from(fresh.len()).unwrap_or(u32::MAX);
    if count > 0 {
      let mut usernames: Vec<String> = Vec::new();
      for (username, _) in &fresh {
        if !usernames.contains(username) && usernames.len() < NAMED {
          usernames.push(username.clone());
        }
      }
      info!(id = %item.id, text = item.search_text, new = count, "wishlist search found new files");
      self
        .live
        .publish(LiveFrame::SearchWishlistFound(WishlistFound {
          item_id: item.id,
          search_text: item.search_text.clone(),
          search_id: search.id,
          new_files: count,
          usernames,
        }));
    }
    RunOutcome {
      ending: JobEnding::Succeeded,
      error: None,
      summary: Some(format!(
        "Searched for \"{}\": {}",
        item.search_text,
        if count == 0 {
          "no new files".to_string()
        } else {
          crate::jobs::counted(count, "new file", "new files")
        }
      )),
      job_id: None,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn a_wishlist_turn_tells_only_of_files_it_had_not_found() {
    let db = crate::db::Database::memory(&super::super::store::SCHEMA).await;
    let item = Uuid::new_v4();
    let at = timestamp(Utc::now());
    let id = item.to_string();
    db.write(move |conn| {
      conn.execute(
        "INSERT INTO wishlist (id, search_text, added_at) VALUES (?1, 'animals', ?2)",
        params![id, at],
      )
    })
    .await
    .unwrap();
    let file = |user: &str, name: &str| (user.to_string(), name.to_string());
    let turn = |found: Vec<(String, String)>| {
      let db = db.clone();
      async move {
        db.write(move |conn| seen(conn, item, Uuid::new_v4(), &found))
          .await
          .unwrap()
      }
    };
    let first = turn(vec![file("alice", "a\\1.flac"), file("bob", "b\\1.flac")]).await;
    assert_eq!(first.len(), 2, "everything is new the first time");
    let second = turn(vec![
      file("alice", "a\\1.flac"),
      file("alice", "a\\2.flac"),
      file("carol", "a\\1.flac"),
    ])
    .await;
    assert_eq!(
      second,
      [file("alice", "a\\2.flac"), file("carol", "a\\1.flac")]
    );
    let items = db.read(list).await.unwrap();
    assert_eq!(items[0].last_new_files, 2);
    assert!(items[0].last_search_id.is_some());
    assert_eq!(db.read(next).await.unwrap().unwrap().id, item);
  }
}
