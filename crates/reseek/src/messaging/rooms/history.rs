//! The rooms in `messaging.db`: every message said in a room while we were in it, and what each
//! room keeps besides: its read marker, what it notifies of, and when we joined and left it.

use api_types::chat::{NotifyLevel, RoomUnread};
use api_types::rooms::{RoomMessage, RoomMessagesQuery, StoredRoom};
use chrono::{DateTime, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use crate::db::{read_timestamp, timestamp};
use crate::messaging::{notify_level, notify_text};

/// The columns [`message`] reads, in its order.
const MESSAGE: &str =
  "local_id, room_name, timestamp, username, message, is_self, is_mention, hidden";

/// A room message as it arrived.
pub struct Arrived {
  /// the room
  pub room: String,
  /// when it arrived
  pub timestamp: DateTime<Utc>,
  /// who said it
  pub username: String,
  /// what they said
  pub message: String,
  /// we said it
  pub is_self: bool,
  /// it mentions us
  pub is_mention: bool,
  /// its author is blocked for rooms
  pub hidden: bool,
}

/// The message in `row`, read with [`MESSAGE`].
fn message(row: &Row) -> rusqlite::Result<RoomMessage> {
  Ok(RoomMessage {
    local_id: row.get(0)?,
    room_name: row.get(1)?,
    timestamp: read_timestamp(&row.get::<_, String>(2)?)?,
    username: row.get(3)?,
    message: row.get(4)?,
    is_self: row.get(5)?,
    is_mention: row.get(6)?,
    hidden: row.get(7)?,
  })
}

/// Stores a room message and returns it with its local id.
pub fn insert(conn: &Connection, arrived: &Arrived) -> rusqlite::Result<RoomMessage> {
  conn.execute(
    "INSERT INTO room_messages (room_name, timestamp, username, message, is_self, is_mention, \
     hidden) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    params![
      arrived.room,
      timestamp(arrived.timestamp),
      arrived.username,
      arrived.message,
      arrived.is_self,
      arrived.is_mention,
      arrived.hidden
    ],
  )?;
  Ok(RoomMessage {
    local_id: conn.last_insert_rowid(),
    room_name: arrived.room.clone(),
    timestamp: arrived.timestamp,
    username: arrived.username.clone(),
    message: arrived.message.clone(),
    is_self: arrived.is_self,
    is_mention: arrived.is_mention,
    hidden: arrived.hidden,
  })
}

/// A page of `room`'s messages, oldest first, and whether older ones exist: the newest
/// `query.limit` before `query.before` when given, else the newest; the hidden ones only with
/// `query.include_hidden`.
pub fn page(
  conn: &Connection,
  room: &str,
  query: &RoomMessagesQuery,
) -> rusqlite::Result<(Vec<RoomMessage>, bool)> {
  let mut filter = String::from("room_name = ?1");
  let mut values = vec![Value::Text(room.to_string())];
  if let Some(before) = query.before {
    values.push(Value::Integer(before));
    filter.push_str(&format!(" AND local_id < ?{}", values.len()));
  }
  if !query.include_hidden {
    filter.push_str(" AND hidden = 0");
  }
  // one more than asked for says whether there are older ones
  values.push(Value::Integer(i64::from(query.limit) + 1));
  let mut page: Vec<RoomMessage> = conn
    .prepare(&format!(
      "SELECT {MESSAGE} FROM room_messages WHERE {filter} ORDER BY local_id DESC LIMIT ?{}",
      values.len()
    ))?
    .query_map(params_from_iter(&values), message)?
    .collect::<rusqlite::Result<_>>()?;
  let has_more = page.len() > query.limit as usize;
  page.truncate(query.limit as usize);
  page.reverse();
  Ok((page, has_more))
}

/// Every message stored for `room`, oldest first.
pub fn all(conn: &Connection, room: &str) -> rusqlite::Result<Vec<RoomMessage>> {
  conn
    .prepare(&format!(
      "SELECT {MESSAGE} FROM room_messages WHERE room_name = ?1 ORDER BY local_id"
    ))?
    .query_map([room], message)?
    .collect()
}

/// When the oldest message stored for `room` arrived, `None` without one.
pub fn oldest(conn: &Connection, room: &str) -> rusqlite::Result<Option<DateTime<Utc>>> {
  let first: Option<String> = conn
    .prepare_cached("SELECT MIN(timestamp) FROM room_messages WHERE room_name = ?1")?
    .query_row([room], |row| row.get(0))?;
  first.as_deref().map(read_timestamp).transpose()
}

/// When the newest message stored for `room` arrived, `None` without one.
pub fn newest(conn: &Connection, room: &str) -> rusqlite::Result<Option<DateTime<Utc>>> {
  let last: Option<String> = conn
    .prepare_cached("SELECT MAX(timestamp) FROM room_messages WHERE room_name = ?1")?
    .query_row([room], |row| row.get(0))?;
  last.as_deref().map(read_timestamp).transpose()
}

/// Every room with stored messages, or only `room`, by name; `joined` says which we are in.
pub fn stored(
  conn: &Connection,
  room: Option<&str>,
  joined: impl Fn(&str) -> bool,
) -> rusqlite::Result<Vec<StoredRoom>> {
  let mut statement = conn.prepare(
    "SELECT room_name, COUNT(*), MIN(timestamp), MAX(timestamp) FROM room_messages \
     WHERE ?1 IS NULL OR room_name = ?1 GROUP BY room_name ORDER BY room_name",
  )?;
  let rows = statement.query_map([room], |row| {
    let name: String = row.get(0)?;
    Ok(StoredRoom {
      joined: joined(&name),
      message_count: row.get::<_, i64>(1)?.max(0).unsigned_abs(),
      first_at: read_timestamp(&row.get::<_, String>(2)?)?,
      last_at: read_timestamp(&row.get::<_, String>(3)?)?,
      name,
    })
  })?;
  rows.collect()
}

/// Deletes every message stored for `room` and returns how many there were.
pub fn delete(conn: &Connection, room: &str) -> rusqlite::Result<usize> {
  conn.execute("DELETE FROM room_messages WHERE room_name = ?1", [room])
}

/// `room`'s messages after its read marker, not ours and not hidden, and those of them that
/// mention us.
pub fn unread(conn: &Connection, room: &str) -> rusqlite::Result<RoomUnread> {
  conn
    .prepare_cached(
      "SELECT COUNT(*), COALESCE(SUM(m.is_mention), 0) FROM room_messages m \
       WHERE m.room_name = ?1 AND m.is_self = 0 AND m.hidden = 0 AND m.local_id > \
       COALESCE((SELECT last_read_id FROM room_state WHERE room_name = ?1), 0)",
    )?
    .query_row([room], |row| {
      let count = |i| -> rusqlite::Result<u32> {
        Ok(u32::try_from(row.get::<_, i64>(i)?).unwrap_or(u32::MAX))
      };
      Ok(RoomUnread {
        unread: count(0)?,
        mentions: count(1)?,
      })
    })
}

/// What `room` notifies of.
pub fn notify(conn: &Connection, room: &str) -> rusqlite::Result<NotifyLevel> {
  let stored: Option<String> = conn
    .prepare_cached("SELECT notify FROM room_state WHERE room_name = ?1")?
    .query_row([room], |row| row.get(0))
    .optional()?;
  stored
    .as_deref()
    .map_or(Ok(NotifyLevel::Mentions), notify_level)
}

/// Notes that we joined `room` at `now`.
pub fn joined(conn: &Connection, room: &str, now: DateTime<Utc>) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO room_state (room_name, joined_at) VALUES (?1, ?2) \
     ON CONFLICT (room_name) DO UPDATE SET joined_at = excluded.joined_at, left_at = NULL",
    params![room, timestamp(now)],
  )?;
  Ok(())
}

/// Notes that we left `room` at `now`.
pub fn left(conn: &Connection, room: &str, now: DateTime<Utc>) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO room_state (room_name, left_at) VALUES (?1, ?2) \
     ON CONFLICT (room_name) DO UPDATE SET left_at = excluded.left_at",
    params![room, timestamp(now)],
  )?;
  Ok(())
}

/// Moves `room`'s read marker to `local_id`, never back.
pub fn mark_read(conn: &Connection, room: &str, local_id: i64) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO room_state (room_name, last_read_id) VALUES (?1, ?2) \
     ON CONFLICT (room_name) DO UPDATE SET last_read_id = max(last_read_id, excluded.last_read_id)",
    params![room, local_id],
  )?;
  Ok(())
}

/// Sets what `room` notifies of.
pub fn set_notify(conn: &Connection, room: &str, notify: NotifyLevel) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO room_state (room_name, notify) VALUES (?1, ?2) \
     ON CONFLICT (room_name) DO UPDATE SET notify = excluded.notify",
    params![room, notify_text(notify)],
  )?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::{Database, Location};

  pub async fn db() -> Database {
    let location = Location {
      data: std::env::temp_dir(),
      backups: std::env::temp_dir(),
      volatile: true,
      log_sql: false,
    };
    Database::open(&crate::messaging::SCHEMA, &location)
      .await
      .unwrap()
  }

  fn said(room: &str, username: &str, at: &str) -> Arrived {
    Arrived {
      room: room.into(),
      timestamp: at.parse().unwrap(),
      username: username.into(),
      message: format!("hello from {username}"),
      is_self: username == "me",
      is_mention: false,
      hidden: false,
    }
  }

  #[tokio::test]
  async fn unread_counts_others_after_the_marker_and_mentions_among_them() {
    let db = db().await;
    let ids = db
      .write(|conn| {
        let mut ids = Vec::new();
        for (who, mention, hidden) in [
          ("alice", false, false),
          ("me", false, false),
          ("bob", true, false),
          ("mallory", true, true),
          ("carol", false, false),
        ] {
          let mut arrived = said("indie", who, "2026-10-26T10:00:00Z");
          arrived.is_mention = mention;
          arrived.hidden = hidden;
          ids.push(insert(conn, &arrived)?.local_id);
        }
        insert(conn, &said("rock", "dave", "2026-10-26T10:00:00Z"))?;
        Ok(ids)
      })
      .await
      .unwrap();
    assert_eq!(
      db.read(|conn| unread(conn, "indie")).await.unwrap(),
      RoomUnread {
        unread: 3,
        mentions: 1
      },
      "ours and the hidden one do not count"
    );
    let marker = ids[2];
    db.write(move |conn| mark_read(conn, "indie", marker))
      .await
      .unwrap();
    let back = ids[0];
    db.write(move |conn| mark_read(conn, "indie", back))
      .await
      .unwrap();
    assert_eq!(
      db.read(|conn| unread(conn, "indie")).await.unwrap(),
      RoomUnread {
        unread: 1,
        mentions: 0
      },
      "the marker never moves back"
    );

    let query = RoomMessagesQuery {
      before: None,
      limit: 2,
      include_hidden: false,
    };
    let (newest, more) = db
      .read(move |conn| page(conn, "indie", &query))
      .await
      .unwrap();
    assert!(more);
    assert_eq!(
      newest
        .iter()
        .map(|m| m.username.as_str())
        .collect::<Vec<_>>(),
      ["bob", "carol"],
      "the newest two, oldest first, without the hidden one"
    );
    let older = RoomMessagesQuery {
      before: Some(newest[0].local_id),
      limit: 10,
      include_hidden: true,
    };
    let (rest, more) = db
      .read(move |conn| page(conn, "indie", &older))
      .await
      .unwrap();
    assert!(!more);
    assert_eq!(rest.len(), 2);
  }
}
