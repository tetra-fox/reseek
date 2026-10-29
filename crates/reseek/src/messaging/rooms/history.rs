//! The rooms in `messaging.db`: every message said in a room while we were in it, and what each
//! room keeps besides: its read marker, what it notifies of, and when we joined and left it.

use api_types::messaging::rooms::{RoomMessage, RoomMessagesQuery, StoredRoom};
use api_types::messaging::{NotifyLevel, RoomUnread};
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

/// When the message stored first for `room` arrived, `None` without one; by local id, as
/// [`newest`] reads the last.
pub fn oldest(conn: &Connection, room: &str) -> rusqlite::Result<Option<DateTime<Utc>>> {
  let first: Option<String> = conn
    .prepare_cached(
      "SELECT timestamp FROM room_messages WHERE room_name = ?1 ORDER BY local_id LIMIT 1",
    )?
    .query_row([room], |row| row.get(0))
    .optional()?;
  first.as_deref().map(read_timestamp).transpose()
}

/// When the message stored last for `room` arrived, `None` without one; by local id, as
/// [`arrived_from`] reads them, so a clock that stepped back does not move it.
pub fn newest(conn: &Connection, room: &str) -> rusqlite::Result<Option<DateTime<Utc>>> {
  let last: Option<String> = conn
    .prepare_cached(
      "SELECT timestamp FROM room_messages WHERE room_name = ?1 ORDER BY local_id DESC LIMIT 1",
    )?
    .query_row([room], |row| row.get(0))
    .optional()?;
  last.as_deref().map(read_timestamp).transpose()
}

/// When the message with `local_id` in `room` arrived, or the first after it when it is gone;
/// `None` when there is neither.
pub fn arrived_from(
  conn: &Connection,
  room: &str,
  local_id: i64,
) -> rusqlite::Result<Option<DateTime<Utc>>> {
  // by local id, not by time, so a clock that stepped back cannot pick an earlier message
  let first: Option<String> = conn
    .prepare_cached(
      "SELECT timestamp FROM room_messages WHERE room_name = ?1 AND local_id >= ?2 \
       ORDER BY local_id LIMIT 1",
    )?
    .query_row(params![room, local_id], |row| row.get(0))
    .optional()?;
  first.as_deref().map(read_timestamp).transpose()
}

/// Every room with stored messages, or only `room`, by name; `joined` says which we are in.
pub fn stored(
  conn: &Connection,
  room: Option<&str>,
  joined: impl Fn(&str) -> bool,
) -> rusqlite::Result<Vec<StoredRoom>> {
  // the first and last by local id, as every read of a room's ends takes them
  let mut statement = conn.prepare(
    "SELECT room_name, COUNT(*), \
     (SELECT timestamp FROM room_messages f WHERE f.room_name = g.room_name \
      ORDER BY local_id LIMIT 1), \
     (SELECT timestamp FROM room_messages l WHERE l.room_name = g.room_name \
      ORDER BY local_id DESC LIMIT 1) \
     FROM room_messages g WHERE ?1 IS NULL OR room_name = ?1 GROUP BY room_name \
     ORDER BY room_name",
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

/// `room`'s read marker: the local id of the newest message shown as read, 0 before any was.
pub fn read_marker(conn: &Connection, room: &str) -> rusqlite::Result<i64> {
  let stored: Option<i64> = conn
    .prepare_cached("SELECT last_read_id FROM room_state WHERE room_name = ?1")?
    .query_row([room], |row| row.get(0))
    .optional()?;
  Ok(stored.unwrap_or(0))
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

/// Notes that we joined `room` at `now`. A room joined because someone asked, `remember`, is
/// joined again after every start until we leave it; a room joined for any other reason keeps
/// whether it was remembered.
pub fn joined(
  conn: &Connection,
  room: &str,
  now: DateTime<Utc>,
  remember: bool,
) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO room_state (room_name, joined_at, remembered) VALUES (?1, ?2, ?3) \
     ON CONFLICT (room_name) DO UPDATE SET joined_at = excluded.joined_at, left_at = NULL, \
     remembered = max(remembered, excluded.remembered)",
    params![room, timestamp(now), remember],
  )?;
  Ok(())
}

/// When we left `room`, `None` when that is not noted or we joined it again since.
pub fn left_at(conn: &Connection, room: &str) -> rusqlite::Result<Option<DateTime<Utc>>> {
  let left: Option<Option<String>> = conn
    .prepare_cached("SELECT left_at FROM room_state WHERE room_name = ?1")?
    .query_row([room], |row| row.get(0))
    .optional()?;
  left.flatten().as_deref().map(read_timestamp).transpose()
}

/// The rooms someone asked to join and nobody left since, in the order of their latest join.
pub fn remembered(conn: &Connection) -> rusqlite::Result<Vec<String>> {
  conn
    .prepare_cached(
      "SELECT room_name FROM room_state WHERE remembered = 1 ORDER BY joined_at, room_name",
    )?
    .query_map([], |row| row.get(0))?
    .collect()
}

/// Notes that we left `room` at `now`, so it is not joined again at the next start.
pub fn left(conn: &Connection, room: &str, now: DateTime<Utc>) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO room_state (room_name, left_at) VALUES (?1, ?2) \
     ON CONFLICT (room_name) DO UPDATE SET left_at = excluded.left_at, remembered = 0",
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
  use crate::db::Database;

  pub async fn db() -> Database {
    Database::memory(&crate::messaging::SCHEMA).await
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
    assert_eq!(db.read(|conn| read_marker(conn, "indie")).await.unwrap(), 0);
    db.write(move |conn| mark_read(conn, "indie", marker))
      .await
      .unwrap();
    assert_eq!(
      db.read(|conn| read_marker(conn, "indie")).await.unwrap(),
      marker
    );
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

  #[tokio::test]
  async fn a_rooms_ends_follow_arrival_when_the_clock_stepped_back() {
    let db = db().await;
    db.write(|conn| {
      // the clock stepped back between the two
      insert(conn, &said("indie", "alice", "2026-10-26T12:00:00Z"))?;
      insert(conn, &said("indie", "bob", "2026-10-26T11:00:00Z"))?;
      Ok(())
    })
    .await
    .unwrap();
    let (rooms, first, last) = db
      .read(|conn| {
        Ok((
          stored(conn, None, |_| false)?,
          oldest(conn, "indie")?,
          newest(conn, "indie")?,
        ))
      })
      .await
      .unwrap();
    let twelve = "2026-10-26T12:00:00Z".parse().unwrap();
    let eleven = "2026-10-26T11:00:00Z".parse().unwrap();
    assert_eq!((rooms[0].first_at, rooms[0].last_at), (twelve, eleven));
    assert_eq!((first, last), (Some(twelve), Some(eleven)));
  }
}
