//! Chat retention: a message goes once it is past its age or its conversation's or room's count,
//! whichever comes first, and a limit of `0` keeps everything. Unacknowledged private messages
//! are never deleted, since the server would deliver them again; a room message that mentions
//! us has an age of its own; a closed conversation left without messages goes with them. The
//! session log is kept back to the oldest room message.

use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use tracing::{info, warn};

use super::Messaging;
use crate::config::MessageRetention;
use crate::db::{shifted, timestamp};

/// `now` less `age`, `None` for an age of `0`, which keeps everything.
fn cutoff(now: DateTime<Utc>, age: Duration) -> Option<String> {
  if age.is_zero() {
    return None;
  }
  // an age too long for chrono keeps everything, as the earliest time stored bounds it
  let age = chrono::Duration::from_std(age).unwrap_or(chrono::Duration::MAX);
  Some(timestamp(shifted(now, -age)))
}

/// Deletes the private messages past `retention` and the closed conversations left without
/// any; returns how many messages went.
pub fn prune_private(
  conn: &mut Connection,
  retention: &MessageRetention,
  now: DateTime<Utc>,
) -> rusqlite::Result<usize> {
  let tx = conn.transaction()?;
  let mut deleted = 0;
  if let Some(before) = cutoff(now, retention.private) {
    deleted += tx.execute(
      "DELETE FROM private_messages WHERE is_acknowledged = 1 AND timestamp < ?1",
      [before],
    )?;
  }
  if retention.max_per_conversation > 0 {
    let cap = retention.max_per_conversation;
    let over: Vec<String> = tx
      .prepare(
        "SELECT username FROM private_messages WHERE is_acknowledged = 1 GROUP BY username \
         HAVING COUNT(*) > ?1",
      )?
      .query_map([cap], |row| row.get(0))?
      .collect::<rusqlite::Result<_>>()?;
    for username in over {
      deleted += tx.execute(
        "DELETE FROM private_messages WHERE username = ?1 AND is_acknowledged = 1 \
         AND local_id <= (SELECT local_id FROM private_messages WHERE username = ?1 \
         AND is_acknowledged = 1 ORDER BY local_id DESC LIMIT 1 OFFSET ?2)",
        params![username, cap],
      )?;
    }
  }
  tx.execute(
    "DELETE FROM conversations WHERE is_active = 0 AND NOT EXISTS \
     (SELECT 1 FROM private_messages m WHERE m.username = conversations.username)",
    [],
  )?;
  tx.commit()?;
  Ok(deleted)
}

/// Deletes the room messages past `retention`, and the sessions older than the oldest room
/// message but the last one; returns how many messages went.
pub fn prune_rooms(
  conn: &mut Connection,
  retention: &MessageRetention,
  now: DateTime<Utc>,
) -> rusqlite::Result<usize> {
  let tx = conn.transaction()?;
  let mut deleted = 0;
  if let Some(before) = cutoff(now, retention.rooms) {
    deleted += tx.execute(
      "DELETE FROM room_messages WHERE is_mention = 0 AND timestamp < ?1",
      [before],
    )?;
  }
  if let Some(before) = cutoff(now, retention.mentions) {
    deleted += tx.execute(
      "DELETE FROM room_messages WHERE is_mention = 1 AND timestamp < ?1",
      [before],
    )?;
  }
  if retention.max_per_room > 0 {
    let cap = retention.max_per_room;
    let over: Vec<String> = tx
      .prepare("SELECT room_name FROM room_messages GROUP BY room_name HAVING COUNT(*) > ?1")?
      .query_map([cap], |row| row.get(0))?
      .collect::<rusqlite::Result<_>>()?;
    for room in over {
      deleted += tx.execute(
        "DELETE FROM room_messages WHERE room_name = ?1 AND local_id <= (SELECT local_id \
         FROM room_messages WHERE room_name = ?1 ORDER BY local_id DESC LIMIT 1 OFFSET ?2)",
        params![room, cap],
      )?;
    }
  }
  // the last ended session is kept: the next login's gap starts at its end
  tx.execute(
    "DELETE FROM sessions WHERE ended_at IS NOT NULL \
     AND id <> (SELECT MAX(id) FROM sessions WHERE ended_at IS NOT NULL) \
     AND ended_at < COALESCE((SELECT MIN(timestamp) FROM room_messages), '9999')",
    [],
  )?;
  tx.commit()?;
  Ok(deleted)
}

impl Messaging {
  /// Every five minutes: the private messages past their retention go.
  pub async fn prune_private(&self) {
    let retention = self.config.current().values.retention.messages.clone();
    match self
      .db
      .write(move |conn| prune_private(conn, &retention, Utc::now()))
      .await
    {
      Ok(0) => {}
      Ok(deleted) => {
        info!(deleted, "private messages past their retention deleted");
        self.unread.changed();
      }
      Err(e) => warn!(error = %e, "private messages past their retention not deleted"),
    }
  }

  /// Every hour: the room messages past their retention go.
  pub async fn prune_rooms(&self) {
    let retention = self.config.current().values.retention.messages.clone();
    match self
      .db
      .write(move |conn| prune_rooms(conn, &retention, Utc::now()))
      .await
    {
      Ok(0) => {}
      Ok(deleted) => {
        info!(deleted, "room messages past their retention deleted");
        self.unread.changed();
      }
      Err(e) => warn!(error = %e, "room messages past their retention not deleted"),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::messaging::conversations::store::{self, Inbound};
  use crate::messaging::rooms::history::{self, Arrived};

  async fn db() -> crate::db::Database {
    crate::db::Database::memory(&crate::messaging::SCHEMA).await
  }

  fn days(n: u64) -> Duration {
    Duration::from_secs(n * 24 * 3600)
  }

  fn at(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
  }

  #[tokio::test]
  async fn rooms_keep_their_newest_and_their_mentions_past_the_age() {
    let db = db().await;
    db.write(|conn| {
      for (n, day) in ["01", "02", "03", "04", "05"].iter().enumerate() {
        history::insert(
          conn,
          &Arrived {
            room: "indie".into(),
            timestamp: at(&format!("2026-01-{day}T00:00:00Z")),
            username: "alice".into(),
            message: format!("message {n}"),
            is_self: false,
            is_mention: n == 0,
            hidden: false,
          },
        )?;
      }
      Ok(())
    })
    .await
    .unwrap();
    let retention = MessageRetention {
      rooms: days(30),
      mentions: days(180),
      max_per_room: 0,
      ..MessageRetention::default()
    };
    let now = at("2026-02-03T00:00:00Z");
    let deleted = db
      .write(move |conn| prune_rooms(conn, &retention, now))
      .await
      .unwrap();
    assert_eq!(
      deleted, 2,
      "days 2 and 3 are older than 30 days; day 1 mentions us"
    );
    let capped = MessageRetention {
      rooms: Duration::ZERO,
      mentions: Duration::ZERO,
      max_per_room: 2,
      ..MessageRetention::default()
    };
    let deleted = db
      .write(move |conn| prune_rooms(conn, &capped, now))
      .await
      .unwrap();
    assert_eq!(deleted, 1, "the count reaches the mention too");
    let left = db.read(|conn| history::all(conn, "indie")).await.unwrap();
    assert_eq!(
      left.iter().map(|m| m.message.as_str()).collect::<Vec<_>>(),
      ["message 3", "message 4"]
    );
  }

  #[tokio::test]
  async fn unacknowledged_private_messages_are_never_deleted() {
    let db = db().await;
    db.write(|conn| {
      store::activate(conn, "alice", at("2026-01-01T00:00:00Z"))?;
      for id in 1..=4 {
        store::receive(
          conn,
          &Inbound {
            id,
            timestamp: at(&format!("2026-01-0{id}T00:00:00Z")),
            username: "alice".into(),
            message: format!("message {id}"),
            replayed: false,
          },
        )?;
      }
      store::acknowledged(conn, "alice", &[1, 2, 3])?;
      store::close(conn, "alice", false, at("2026-01-05T00:00:00Z"))?;
      Ok(())
    })
    .await
    .unwrap();
    let capped = MessageRetention {
      private: Duration::ZERO,
      max_per_conversation: 1,
      ..MessageRetention::default()
    };
    let now = at("2026-06-01T00:00:00Z");
    let deleted = db
      .write(move |conn| prune_private(conn, &capped, now))
      .await
      .unwrap();
    assert_eq!(deleted, 2, "the newest read one stays with the unread one");
    let aged = MessageRetention {
      private: days(90),
      max_per_conversation: 0,
      ..MessageRetention::default()
    };
    db.write(move |conn| prune_private(conn, &aged, now))
      .await
      .unwrap();
    let left = db.read(|conn| store::all(conn, "alice")).await.unwrap();
    assert_eq!(
      left.iter().map(|m| m.id).collect::<Vec<_>>(),
      [Some(4)],
      "the unread message outlives every limit"
    );
    assert!(
      db.read(|conn| store::exists(conn, "alice")).await.unwrap(),
      "a closed conversation with a message left stays"
    );
  }
}
