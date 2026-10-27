//! Searching what was said: a full-text query over the private messages, the room messages, or
//! both, newest first, each hit with the matching part of its text marked. Room messages of
//! blocked users are left out, as the rooms show them collapsed.

use api_types::chat::{
  MessageHit, MessageHitKind, MessageSearchKind, MessageSearchQuery, MessageSearchResults,
};
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use super::Messaging;
use crate::api::error::ApiError;
use crate::db::{DbError, read_timestamp};

/// Where a snippet's match starts and ends before it is escaped; no message holds them.
const OPEN: &str = "\u{1}";
const CLOSE: &str = "\u{2}";

/// How many words a snippet holds around a match.
const SNIPPET_WORDS: u32 = 12;

/// `text` with what HTML reads as markup escaped, and the match markers turned into `<mark>`.
fn snippet_html(text: &str) -> String {
  let mut escaped = String::with_capacity(text.len());
  for c in text.chars() {
    match c {
      '&' => escaped.push_str("&amp;"),
      '<' => escaped.push_str("&lt;"),
      '>' => escaped.push_str("&gt;"),
      '"' => escaped.push_str("&quot;"),
      '\'' => escaped.push_str("&#39;"),
      c => escaped.push(c),
    }
  }
  escaped.replace(OPEN, "<mark>").replace(CLOSE, "</mark>")
}

/// The statement finding the hits `query` asks for, with its values; `None` when no kind of
/// message can match.
fn statement(query: &MessageSearchQuery) -> Option<(String, Vec<Value>)> {
  let mut values = vec![Value::Text(query.q.clone())];
  let snippet =
    |table: &str| format!("snippet({table}, 0, '{OPEN}', '{CLOSE}', '…', {SNIPPET_WORDS})");
  let mut parts = Vec::new();
  if query.kind != MessageSearchKind::Rooms && query.room.is_none() {
    let mut filter = String::new();
    if let Some(username) = &query.username {
      values.push(Value::Text(username.clone()));
      filter = format!(" AND m.username = ?{}", values.len());
    }
    parts.push(format!(
      "SELECT 'private', m.username, NULL, m.timestamp, m.message, m.local_id, {} \
       FROM private_messages_fts JOIN private_messages m ON m.local_id = private_messages_fts.rowid \
       WHERE private_messages_fts MATCH ?1{filter}",
      snippet("private_messages_fts")
    ));
  }
  if query.kind != MessageSearchKind::Private {
    let mut filter = String::from(" AND m.hidden = 0");
    if let Some(room) = &query.room {
      values.push(Value::Text(room.clone()));
      filter.push_str(&format!(" AND m.room_name = ?{}", values.len()));
    }
    if let Some(username) = &query.username {
      values.push(Value::Text(username.clone()));
      filter.push_str(&format!(" AND m.username = ?{}", values.len()));
    }
    parts.push(format!(
      "SELECT 'room', m.username, m.room_name, m.timestamp, m.message, m.local_id, {} \
       FROM room_messages_fts JOIN room_messages m ON m.local_id = room_messages_fts.rowid \
       WHERE room_messages_fts MATCH ?1{filter}",
      snippet("room_messages_fts")
    ));
  }
  (!parts.is_empty()).then(|| (parts.join(" UNION ALL "), values))
}

/// The hits `query` asks for, and how many there are on every page.
fn search(conn: &Connection, query: &MessageSearchQuery) -> rusqlite::Result<MessageSearchResults> {
  let Some((hits, mut values)) = statement(query) else {
    return Ok(MessageSearchResults {
      hits: Vec::new(),
      total: 0,
    });
  };
  let total: i64 = conn.query_row(
    &format!("SELECT COUNT(*) FROM ({hits})"),
    params_from_iter(&values),
    |row| row.get(0),
  )?;
  values.push(Value::Integer(i64::from(query.limit)));
  values.push(Value::Integer(i64::from(query.offset)));
  let page = format!(
    "SELECT * FROM ({hits}) ORDER BY 4 DESC, 6 DESC LIMIT ?{} OFFSET ?{}",
    values.len() - 1,
    values.len()
  );
  let hits = conn
    .prepare(&page)?
    .query_map(params_from_iter(&values), |row| {
      let kind = match row.get_ref(0)?.as_str()? {
        "room" => MessageHitKind::Room,
        _ => MessageHitKind::Private,
      };
      Ok(MessageHit {
        kind,
        username: row.get(1)?,
        room: row.get(2)?,
        timestamp: read_timestamp(&row.get::<_, String>(3)?)?,
        message: row.get(4)?,
        local_id: row.get(5)?,
        snippet: snippet_html(&row.get::<_, String>(6)?),
      })
    })?
    .collect::<rusqlite::Result<_>>()?;
  Ok(MessageSearchResults {
    hits,
    total: total.max(0).unsigned_abs(),
  })
}

/// Runs `q` against an empty full-text index in memory, with the message tables' column and
/// tokenizer, so a failure there is the query's own and never the database's.
fn check_query(q: &str) -> rusqlite::Result<()> {
  let conn = Connection::open_in_memory()?;
  conn.execute_batch("CREATE VIRTUAL TABLE probe USING fts5(message, tokenize = 'unicode61')")?;
  conn.query_row(
    "SELECT COUNT(*) FROM probe WHERE probe MATCH ?1",
    [q],
    |_| Ok(()),
  )
}

impl Messaging {
  pub(super) async fn search(
    &self,
    query: MessageSearchQuery,
  ) -> Result<MessageSearchResults, ApiError> {
    let q = query.q.clone();
    let checked = tokio::task::spawn_blocking(move || check_query(&q))
      .await
      .map_err(|e| ApiError::internal(DbError::Task(e.to_string())))?;
    if let Err(e) = checked {
      return Err(ApiError::bad_request(format!(
        "q is not a valid search: {e}"
      )));
    }
    self
      .db
      .read(move |conn| search(conn, &query))
      .await
      .map_err(ApiError::internal)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::messaging::conversations::store::{self, Inbound};
  use crate::messaging::rooms::history::{self, Arrived};

  fn query(q: &str, kind: MessageSearchKind) -> MessageSearchQuery {
    MessageSearchQuery {
      q: q.into(),
      kind,
      room: None,
      username: None,
      offset: 0,
      limit: 50,
    }
  }

  #[tokio::test]
  async fn private_and_room_messages_are_found_with_their_match_marked() {
    let location = crate::db::Location {
      data: std::env::temp_dir(),
      backups: std::env::temp_dir(),
      volatile: true,
      log_sql: false,
    };
    let db = crate::db::Database::open(&crate::messaging::SCHEMA, &location)
      .await
      .unwrap();
    db.write(|conn| {
      store::receive(
        conn,
        &Inbound {
          id: 1,
          timestamp: "2026-10-26T10:00:00Z".parse().unwrap(),
          username: "alice".into(),
          message: "have you heard <Animals> by pink floyd?".into(),
          replayed: false,
        },
      )?;
      for (who, text, hidden) in [
        ("bob", "Pink Floyd live tonight", false),
        ("mallory", "pink floyd spam", true),
      ] {
        history::insert(
          conn,
          &Arrived {
            room: "indie".into(),
            timestamp: "2026-10-26T11:00:00Z".parse().unwrap(),
            username: who.into(),
            message: text.into(),
            is_self: false,
            is_mention: false,
            hidden,
          },
        )?;
      }
      Ok(())
    })
    .await
    .unwrap();
    let found = db
      .read(|conn| search(conn, &query("\"pink floyd\"", MessageSearchKind::All)))
      .await
      .unwrap();
    assert_eq!(found.total, 2, "the hidden message is left out");
    assert_eq!(found.hits[0].kind, MessageHitKind::Room, "newest first");
    assert_eq!(found.hits[0].room.as_deref(), Some("indie"));
    let private = &found.hits[1];
    assert_eq!(private.username, "alice");
    assert_eq!(
      private.snippet,
      "have you heard &lt;Animals&gt; by <mark>pink floyd</mark>?"
    );
    let rooms_only = db
      .read(|conn| search(conn, &query("floyd", MessageSearchKind::Rooms)))
      .await
      .unwrap();
    assert_eq!(rooms_only.total, 1);
    assert!(check_query("\"pink floyd\" OR message:animals").is_ok());
    assert!(
      check_query("\"open").is_err(),
      "an unclosed phrase is not a query"
    );
    assert!(check_query("nosuchcolumn:x").is_err());
  }

  #[tokio::test]
  async fn a_query_sqlite_cannot_read_is_the_callers_mistake() {
    let app = crate::testing::app(
      "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    )
    .await;
    let get =
      |q: &str| crate::testing::request("GET", &format!("/api/v0/messages/search?q={q}"), None);
    let bad = app.send(get("%22open")).await;
    assert_eq!(bad.status, 400, "{:?}", bad.body);
    assert_eq!(app.send(get("floyd")).await.status, 200);
  }
}
