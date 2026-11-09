//! The private conversations in `messaging.db`: the conversations, their messages, and what
//! each notifies of and keeps as a draft.

use std::collections::BTreeMap;

use api_types::messaging::NotifyLevel;
use api_types::messaging::conversations::{
  Conversation, ConversationListQuery, ConversationSummary, MessageDirection, PrivateMessage,
};
use api_types::users::MessageCounts;
use chrono::{DateTime, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, named_params, params, params_from_iter};
pub use soulseek_proto::SERVER_USERNAME as SERVER;

use crate::db::{read_timestamp, timestamp};
use crate::messaging::{notify_level, notify_text};

/// The condition, on a `username` column, that leaves out the conversation holding the
/// server's own notices, which are never unread; the name is bound as `:server`.
macro_rules! not_server {
  () => {
    "username <> :server"
  };
}

/// An inbound message not acknowledged, which counts as unread unless it is a server notice;
/// it binds `:server`.
macro_rules! unread {
  () => {
    concat!("direction = 1 AND is_acknowledged = 0 AND ", not_server!())
  };
}

const UNREAD: &str = unread!();

/// The columns [`message_from`] reads, in its order, each behind `$table` (such as `"l."`) when
/// the query joins more than one table.
macro_rules! message_columns {
  ($table:literal) => {
    concat!(
      $table,
      "local_id, ",
      $table,
      "remote_id, ",
      $table,
      "timestamp, ",
      $table,
      "username, ",
      $table,
      "direction, ",
      $table,
      "message, ",
      $table,
      "is_acknowledged, ",
      $table,
      "was_replayed, ",
      $table,
      "auto_reply"
    )
  };
}

/// The columns [`message`] reads, in its order.
const MESSAGE: &str = message_columns!("");

/// A private message as it arrived.
pub struct Inbound {
  /// the server's id for it
  pub id: u32,
  /// when the server says it was sent
  pub timestamp: DateTime<Utc>,
  /// who sent it
  pub username: String,
  /// the text
  pub message: String,
  /// the server delivered it from its offline store at a login, not live
  pub replayed: bool,
}

/// The direction in the column `i` of `row`, stored as 0 for out and 1 for in; another
/// number is an error that names the message `local_id`.
fn direction(row: &Row, i: usize, local_id: i64) -> rusqlite::Result<MessageDirection> {
  match row.get::<_, i64>(i)? {
    0 => Ok(MessageDirection::Out),
    1 => Ok(MessageDirection::In),
    other => Err(rusqlite::Error::FromSqlConversionFailure(
      i,
      rusqlite::types::Type::Integer,
      format!("private message {local_id} has the direction {other}, which is neither 0 nor 1")
        .into(),
    )),
  }
}

/// The message in `row`, read with [`MESSAGE`].
fn message(row: &Row) -> rusqlite::Result<PrivateMessage> {
  message_from(row, 0)
}

/// The message in `row` whose [`MESSAGE`] columns start at `at`.
fn message_from(row: &Row, at: usize) -> rusqlite::Result<PrivateMessage> {
  let local_id = row.get(at)?;
  Ok(PrivateMessage {
    local_id,
    id: row.get(at + 1)?,
    timestamp: read_timestamp(&row.get::<_, String>(at + 2)?)?,
    username: row.get(at + 3)?,
    direction: direction(row, at + 4, local_id)?,
    message: row.get(at + 5)?,
    is_acknowledged: row.get(at + 6)?,
    // written once, at the first delivery: it came from the server's offline store, so it was
    // sent while we were not logged in
    was_replayed: row.get(at + 7)?,
    auto_reply: row.get(at + 8)?,
  })
}

/// The message with `local_id`.
fn message_at(conn: &Connection, local_id: i64) -> rusqlite::Result<PrivateMessage> {
  conn
    .prepare_cached(&format!(
      "SELECT {MESSAGE} FROM private_messages WHERE local_id = ?1"
    ))?
    .query_row([local_id], message)
}

/// Opens the conversation with `username`, making it when there is none.
pub fn activate(conn: &Connection, username: &str, now: DateTime<Utc>) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO conversations (username, is_active, created_at, updated_at) \
     VALUES (?1, 1, ?2, ?2) \
     ON CONFLICT (username) DO UPDATE SET is_active = 1, updated_at = excluded.updated_at",
    params![username, timestamp(now)],
  )?;
  Ok(())
}

/// Makes the conversation with `username` when there is none, leaving one that exists as it is.
pub fn ensure(conn: &Connection, username: &str, now: DateTime<Utc>) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO conversations (username, is_active, created_at, updated_at) \
     VALUES (?1, 1, ?2, ?2) ON CONFLICT (username) DO NOTHING",
    params![username, timestamp(now)],
  )?;
  Ok(())
}

/// Stores an inbound message, giving it as stored and whether this delivery stored it. A message
/// the server delivers again is the one stored already: it is marked unacknowledged, its text
/// and whether it came from the offline store left as the first delivery set them.
pub fn receive(conn: &Connection, inbound: &Inbound) -> rusqlite::Result<(PrivateMessage, bool)> {
  let stored: Option<(i64, String)> = conn
    .prepare_cached(
      "SELECT local_id, message FROM private_messages WHERE username = ?1 AND remote_id = ?2",
    )?
    .query_row(params![inbound.username, inbound.id], |row| {
      Ok((row.get(0)?, row.get(1)?))
    })
    .optional()?;
  let (local_id, new) = match stored {
    Some((local_id, text)) => {
      if text != inbound.message {
        // a server that started its ids over; one id holds one message here
        tracing::warn!(
          username = inbound.username,
          id = inbound.id,
          "private message dropped: its id is a stored message's with another text"
        );
      }
      conn.execute(
        "UPDATE private_messages SET is_acknowledged = 0 WHERE local_id = ?1",
        [local_id],
      )?;
      (local_id, false)
    }
    None => {
      conn.execute(
        "INSERT INTO private_messages (username, remote_id, timestamp, direction, message, \
         is_acknowledged, was_replayed) VALUES (?1, ?2, ?3, 1, ?4, 0, ?5)",
        params![
          inbound.username,
          inbound.id,
          timestamp(inbound.timestamp),
          inbound.message,
          inbound.replayed
        ],
      )?;
      (conn.last_insert_rowid(), true)
    }
  };
  Ok((message_at(conn, local_id)?, new))
}

/// Stores a message we sent to `username` at `now`; `auto_reply` when it answered someone on
/// its own while we were away.
pub fn sent(
  conn: &Connection,
  username: &str,
  text: &str,
  auto_reply: bool,
  now: DateTime<Utc>,
) -> rusqlite::Result<PrivateMessage> {
  conn.execute(
    "INSERT INTO private_messages (username, remote_id, timestamp, direction, message, \
     is_acknowledged, was_replayed, auto_reply) VALUES (?1, NULL, ?2, 0, ?3, 1, 0, ?4)",
    params![username, timestamp(now), text, auto_reply],
  )?;
  message_at(conn, conn.last_insert_rowid())
}

/// The server's ids of the inbound messages from `username` not acknowledged yet.
pub fn unacknowledged(conn: &Connection, username: &str) -> rusqlite::Result<Vec<u32>> {
  conn
    .prepare_cached(
      "SELECT remote_id FROM private_messages WHERE username = ?1 AND direction = 1 \
       AND is_acknowledged = 0 AND remote_id IS NOT NULL ORDER BY local_id",
    )?
    .query_map([username], |row| row.get(0))?
    .collect()
}

/// Marks the messages from `username` with the server's `ids` acknowledged.
pub fn acknowledged(conn: &mut Connection, username: &str, ids: &[u32]) -> rusqlite::Result<()> {
  let tx = conn.transaction()?;
  {
    let mut statement = tx.prepare_cached(
      "UPDATE private_messages SET is_acknowledged = 1 WHERE username = ?1 AND remote_id = ?2",
    )?;
    for id in ids {
      statement.execute(params![username, id])?;
    }
  }
  tx.commit()
}

/// Whether the inbound message from `username` with the server's `id` is stored.
pub fn has_message(conn: &Connection, username: &str, id: u32) -> rusqlite::Result<bool> {
  conn
    .prepare_cached(
      "SELECT EXISTS (SELECT 1 FROM private_messages WHERE username = ?1 AND remote_id = ?2)",
    )?
    .query_row(params![username, id], |row| row.get(0))
}

/// Whether there is a conversation with `username`, open or closed.
pub fn exists(conn: &Connection, username: &str) -> rusqlite::Result<bool> {
  conn
    .prepare_cached("SELECT EXISTS (SELECT 1 FROM conversations WHERE username = ?1)")?
    .query_row([username], |row| row.get(0))
}

/// The conversation columns [`conversation`] reads, joined with their state, their count of
/// unread messages and their newest message's [`MESSAGE`] columns; it binds `:server`.
const CONVERSATION: &str = concat!(
  "SELECT c.username, c.is_active, COALESCE(s.notify, 'all'), COALESCE(s.draft, ''), \
   (SELECT COUNT(*) FROM private_messages m WHERE m.username = c.username AND ",
  unread!(),
  ") AS unacknowledged, ",
  message_columns!("l."),
  " FROM conversations c LEFT JOIN conversation_state s ON s.username = c.username \
   LEFT JOIN private_messages l ON l.local_id = \
   (SELECT MAX(local_id) FROM private_messages n WHERE n.username = c.username)"
);

/// The conversation in `row`, read with [`CONVERSATION`], without its messages.
fn conversation_of(row: &Row) -> rusqlite::Result<Conversation> {
  let username: String = row.get(0)?;
  let unacknowledged = u32::try_from(row.get::<_, i64>(4)?).unwrap_or(u32::MAX);
  Ok(Conversation {
    is_server_notice: username == SERVER,
    username,
    is_active: row.get(1)?,
    un_acknowledged_message_count: unacknowledged,
    has_un_acknowledged_messages: unacknowledged > 0,
    notify: notify_level(&row.get::<_, String>(2)?)?,
    draft: row.get(3)?,
    last_message: match row.get::<_, Option<i64>>(5)? {
      Some(_) => Some(message_from(row, 5)?),
      None => None,
    },
    messages: None,
  })
}

/// The conversation with `username`, without its messages.
pub fn conversation(conn: &Connection, username: &str) -> rusqlite::Result<Option<Conversation>> {
  conn
    .prepare_cached(&format!("{CONVERSATION} WHERE c.username = :username"))?
    .query_row(
      named_params! { ":username": username, ":server": SERVER },
      conversation_of,
    )
    .optional()
}

/// The conversations `query` asks for, by username.
pub fn list(
  conn: &Connection,
  query: &ConversationListQuery,
) -> rusqlite::Result<Vec<Conversation>> {
  let mut filter = vec!["1 = 1"];
  if !query.include_inactive {
    filter.push("c.is_active = 1");
  }
  if !query.include_server {
    filter.push(concat!("c.", not_server!()));
  }
  let having = if query.un_acknowledged_only {
    "WHERE unacknowledged > 0"
  } else {
    ""
  };
  conn
    .prepare(&format!(
      "SELECT * FROM ({CONVERSATION} WHERE {}) {having} ORDER BY username",
      filter.join(" AND ")
    ))?
    .query_map(named_params! { ":server": SERVER }, conversation_of)?
    .collect()
}

/// A page of the messages with `username`, oldest first: the oldest `limit` after the message
/// `after` when given, else the newest `limit` before the message `before` when given, else the
/// newest, only the unacknowledged ones with `unacknowledged`.
pub fn messages(
  conn: &Connection,
  username: &str,
  before: Option<i64>,
  after: Option<i64>,
  limit: u32,
  unacknowledged: bool,
) -> rusqlite::Result<Vec<PrivateMessage>> {
  let mut filter = String::from("username = ?1");
  let mut values = vec![Value::Text(username.to_string())];
  if let Some(before) = before {
    values.push(Value::Integer(before));
    filter.push_str(&format!(" AND local_id < ?{}", values.len()));
  }
  if let Some(after) = after {
    values.push(Value::Integer(after));
    filter.push_str(&format!(" AND local_id > ?{}", values.len()));
  }
  if unacknowledged {
    filter.push_str(" AND direction = 1 AND is_acknowledged = 0");
  }
  values.push(Value::Integer(i64::from(limit)));
  let order = if after.is_some() { "ASC" } else { "DESC" };
  let mut page: Vec<PrivateMessage> = conn
    .prepare(&format!(
      "SELECT {MESSAGE} FROM private_messages WHERE {filter} ORDER BY local_id {order} LIMIT ?{}",
      values.len()
    ))?
    .query_map(params_from_iter(&values), message)?
    .collect::<rusqlite::Result<_>>()?;
  if after.is_none() {
    page.reverse();
  }
  Ok(page)
}

/// Every message with `username`, oldest first.
pub fn all(conn: &Connection, username: &str) -> rusqlite::Result<Vec<PrivateMessage>> {
  conn
    .prepare(&format!(
      "SELECT {MESSAGE} FROM private_messages WHERE username = ?1 ORDER BY local_id"
    ))?
    .query_map([username], message)?
    .collect()
}

/// Closes the conversation with `username`; with `purge` its messages are deleted too.
pub fn close(
  conn: &mut Connection,
  username: &str,
  purge: bool,
  now: DateTime<Utc>,
) -> rusqlite::Result<()> {
  let tx = conn.transaction()?;
  tx.execute(
    "UPDATE conversations SET is_active = 0, updated_at = ?2 WHERE username = ?1",
    params![username, timestamp(now)],
  )?;
  if purge {
    tx.execute(
      "DELETE FROM private_messages WHERE username = ?1",
      [username],
    )?;
  }
  tx.commit()
}

/// Sets what the conversation with `username` notifies of.
pub fn set_notify(conn: &Connection, username: &str, notify: NotifyLevel) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO conversation_state (username, notify) VALUES (?1, ?2) \
     ON CONFLICT (username) DO UPDATE SET notify = excluded.notify",
    params![username, notify_text(notify)],
  )?;
  Ok(())
}

/// Keeps `draft` as the unsent text of the conversation with `username`.
pub fn set_draft(conn: &Connection, username: &str, draft: &str) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO conversation_state (username, draft) VALUES (?1, ?2) \
     ON CONFLICT (username) DO UPDATE SET draft = excluded.draft",
    params![username, draft],
  )?;
  Ok(())
}

/// The counts of our messages with `username`, and when the latest was sent.
pub fn figures(
  conn: &Connection,
  username: &str,
) -> rusqlite::Result<(MessageCounts, Option<DateTime<Utc>>)> {
  conn
    .prepare_cached(&format!(
      "SELECT COALESCE(SUM(direction = 1), 0), COALESCE(SUM(direction = 0), 0), \
       COALESCE(SUM({UNREAD}), 0), MAX(timestamp) \
       FROM private_messages WHERE username = :username"
    ))?
    .query_row(
      named_params! { ":username": username, ":server": SERVER },
      |row| {
        let count = |i| -> rusqlite::Result<u32> {
          Ok(u32::try_from(row.get::<_, i64>(i)?).unwrap_or(u32::MAX))
        };
        let latest: Option<String> = row.get(3)?;
        Ok((
          MessageCounts {
            inbound: count(0)?,
            outbound: count(1)?,
            unread: count(2)?,
          },
          latest.as_deref().map(read_timestamp).transpose()?,
        ))
      },
    )
}

/// The state of the conversation with `username`, when there is one.
pub fn summary(conn: &Connection, username: &str) -> rusqlite::Result<Option<ConversationSummary>> {
  Ok(conversation(conn, username)?.map(|c| ConversationSummary {
    is_active: c.is_active,
    un_acknowledged_message_count: c.un_acknowledged_message_count,
  }))
}

/// The unacknowledged messages of every conversation that has any, by peer.
pub fn unread(conn: &Connection) -> rusqlite::Result<BTreeMap<String, u32>> {
  conn
    .prepare_cached(&format!(
      "SELECT username, COUNT(*) FROM private_messages WHERE {UNREAD} GROUP BY username"
    ))?
    .query_map(named_params! { ":server": SERVER }, |row| {
      Ok((
        row.get::<_, String>(0)?,
        u32::try_from(row.get::<_, i64>(1)?).unwrap_or(u32::MAX),
      ))
    })?
    .collect()
}

/// The peers of the open conversations.
pub fn active(conn: &Connection) -> rusqlite::Result<Vec<String>> {
  conn
    .prepare(concat!(
      "SELECT username FROM conversations WHERE is_active = 1 AND ",
      not_server!()
    ))?
    .query_map(named_params! { ":server": SERVER }, |row| row.get(0))?
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::Database;

  #[tokio::test]
  async fn the_servers_notices_are_never_unread() {
    let db = Database::memory(&crate::messaging::SCHEMA).await;
    let now: DateTime<Utc> = "2026-10-26T10:00:00Z".parse().unwrap();
    db.write(move |conn| {
      for (id, username) in [(1, SERVER), (2, "alice")] {
        activate(conn, username, now)?;
        receive(
          conn,
          &Inbound {
            id,
            timestamp: now,
            username: username.into(),
            message: "hello".into(),
            replayed: false,
          },
        )?;
      }
      Ok(())
    })
    .await
    .unwrap();
    let (unread_now, notices, figures, listed, everything, open) = db
      .read(|conn| {
        let all = ConversationListQuery {
          include_inactive: true,
          un_acknowledged_only: false,
          include_server: true,
        };
        Ok((
          unread(conn)?,
          conversation(conn, SERVER)?.unwrap(),
          figures(conn, SERVER)?.0,
          list(conn, &ConversationListQuery::default())?,
          list(conn, &all)?,
          active(conn)?,
        ))
      })
      .await
      .unwrap();
    assert_eq!(everything.len(), 2, "listed when asked for");
    assert_eq!(unread_now, BTreeMap::from([("alice".to_string(), 1)]));
    assert!(notices.is_server_notice);
    assert_eq!(notices.un_acknowledged_message_count, 0);
    assert_eq!((figures.inbound, figures.unread), (1, 0));
    assert_eq!(listed.len(), 1, "left out unless asked for");
    assert_eq!(listed[0].un_acknowledged_message_count, 1);
    assert_eq!(open, ["alice"]);
  }

  #[tokio::test]
  async fn a_message_received_live_stays_live_when_delivered_again() {
    let db = Database::memory(&crate::messaging::SCHEMA).await;
    let now: DateTime<Utc> = "2026-10-26T14:00:00Z".parse().unwrap();
    let delivery = move |replayed| Inbound {
      id: 7,
      timestamp: now,
      username: "alice".into(),
      message: "hello".into(),
      replayed,
    };
    let (live, again) = db
      .write(move |conn| {
        activate(conn, "alice", now)?;
        // left unread, so the server delivers it again at the next login
        let (live, _) = receive(conn, &delivery(false))?;
        Ok((live, receive(conn, &delivery(true))?.0))
      })
      .await
      .unwrap();
    assert_eq!(again.local_id, live.local_id);
    assert!(!again.was_replayed, "it arrived while we were logged in");
  }

  #[tokio::test]
  async fn a_conversation_carries_its_newest_message_either_way() {
    let db = Database::memory(&crate::messaging::SCHEMA).await;
    let at = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
    db.write(move |conn| {
      activate(conn, "alice", at("2026-10-26T10:00:00Z"))?;
      activate(conn, "bob", at("2026-10-26T10:00:00Z"))?;
      receive(
        conn,
        &Inbound {
          id: 7,
          timestamp: at("2026-10-26T10:00:00Z"),
          username: "alice".into(),
          message: "hello".into(),
          replayed: false,
        },
      )?;
      sent(conn, "alice", "hi back", false, at("2026-10-26T10:01:00Z"))?;
      Ok(())
    })
    .await
    .unwrap();
    let (listed, one) = db
      .read(|conn| {
        Ok((
          list(conn, &ConversationListQuery::default())?,
          conversation(conn, "alice")?.unwrap(),
        ))
      })
      .await
      .unwrap();
    let last = one.last_message.unwrap();
    assert_eq!(
      (last.message.as_str(), last.direction),
      ("hi back", MessageDirection::Out),
      "the newest, ours"
    );
    assert_eq!(listed[0].username, "alice");
    assert_eq!(
      listed[0].last_message.as_ref().unwrap().local_id,
      last.local_id
    );
    assert!(listed[1].last_message.is_none(), "bob has none");
    assert_eq!(listed[0].un_acknowledged_message_count, 1);
  }

  #[tokio::test]
  async fn a_page_after_a_message_holds_the_oldest_past_it_in_order() {
    let db = Database::memory(&crate::messaging::SCHEMA).await;
    let (ids, page) = db
      .write(|conn| {
        let now = Utc::now();
        activate(conn, "alice", now)?;
        let ids = (0..4)
          .map(|n| Ok(sent(conn, "alice", &format!("line {n}"), false, now)?.local_id))
          .collect::<rusqlite::Result<Vec<_>>>()?;
        let page = messages(conn, "alice", None, Some(ids[0]), 2, false)?;
        Ok((ids, page))
      })
      .await
      .unwrap();
    let read: Vec<_> = page.iter().map(|m| m.local_id).collect();
    assert_eq!(read, ids[1..3]);
  }

  #[tokio::test]
  async fn the_newest_message_is_found_by_index() {
    let db = Database::memory(&crate::messaging::SCHEMA).await;
    let plan = db
      .read(|conn| {
        conn
          .prepare(&format!(
            "EXPLAIN QUERY PLAN {CONVERSATION} WHERE c.username = :username"
          ))?
          .query_map(
            named_params! { ":username": "alice", ":server": SERVER },
            |row| row.get::<_, String>(3),
          )?
          .collect::<rusqlite::Result<Vec<_>>>()
      })
      .await
      .unwrap();
    assert!(
      plan
        .iter()
        .any(|step| step.contains("COVERING INDEX pm_user_local")),
      "{plan:#?}"
    );
  }
}
