//! The chats pinned above the others in the chat list, rooms and users in one order of the
//! user's. A pin is a setting of the chat, so it stays while the room is left or the conversation
//! closed, the list then leaving it out until the chat is back.

use api_types::live::LiveFrame;
use api_types::messaging::{ChatKind, ChatPin};
use rusqlite::{Connection, params};
use tracing::debug;

use super::Messaging;
use crate::api::error::ApiError;

/// A chat kind as stored.
fn kind_text(kind: ChatKind) -> &'static str {
  match kind {
    ChatKind::Room => "room",
    ChatKind::User => "user",
  }
}

/// A chat kind [`kind_text`] stored.
fn chat_kind(text: &str) -> rusqlite::Result<ChatKind> {
  match text {
    "room" => Ok(ChatKind::Room),
    "user" => Ok(ChatKind::User),
    other => Err(rusqlite::Error::FromSqlConversionFailure(
      0,
      rusqlite::types::Type::Text,
      format!("not a chat kind: {other}").into(),
    )),
  }
}

/// The pinned chats in the user's order.
fn list(conn: &Connection) -> rusqlite::Result<Vec<ChatPin>> {
  conn
    .prepare_cached("SELECT kind, name FROM chat_pins ORDER BY position")?
    .query_map([], |row| {
      Ok(ChatPin {
        kind: chat_kind(row.get_ref(0)?.as_str()?)?,
        name: row.get(1)?,
      })
    })?
    .collect()
}

/// Makes `pins`, each named once, the pinned chats in that order.
fn replace(conn: &mut Connection, pins: &[ChatPin]) -> rusqlite::Result<()> {
  let tx = conn.transaction()?;
  tx.execute("DELETE FROM chat_pins", [])?;
  {
    let mut insert =
      tx.prepare_cached("INSERT INTO chat_pins (kind, name, position) VALUES (?1, ?2, ?3)")?;
    for (position, pin) in (0_i64..).zip(pins) {
      insert.execute(params![kind_text(pin.kind), pin.name, position])?;
    }
  }
  tx.commit()
}

impl Messaging {
  pub(super) async fn pins(&self) -> Result<Vec<ChatPin>, ApiError> {
    self.db.read(list).await.map_err(ApiError::internal)
  }

  pub(super) async fn set_pins(&self, pins: Vec<ChatPin>) -> Result<(), ApiError> {
    let pins = self
      .db
      .write(move |conn| {
        replace(conn, &pins)?;
        Ok(pins)
      })
      .await
      .map_err(ApiError::internal)?;
    debug!(pins = pins.len(), "chat pins set");
    self.live.publish(LiveFrame::ChatPins(pins));
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::{self, Database, Location, Schema};
  use crate::messaging::SCHEMA;

  /// `messaging.db` as it was before pins.
  static BEFORE: Schema = db::schema!(
    "messaging",
    ["0001_initial", "0002_drop_session_end_reason"]
  );

  fn location(dir: &tempfile::TempDir) -> Location {
    let location = Location {
      data: dir.path().join("data"),
      backups: dir.path().join("backups"),
      volatile: false,
      log_sql: false,
    };
    std::fs::create_dir_all(&location.data).unwrap();
    std::fs::create_dir_all(&location.backups).unwrap();
    location
  }

  fn room(name: &str) -> ChatPin {
    ChatPin {
      kind: ChatKind::Room,
      name: name.to_string(),
    }
  }

  fn user(name: &str) -> ChatPin {
    ChatPin {
      kind: ChatKind::User,
      name: name.to_string(),
    }
  }

  #[tokio::test]
  async fn pins_keep_their_order_across_a_reorder_and_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let location = location(&dir);
    let db = Database::open(&SCHEMA, &location).await.unwrap();
    // a room and a user of the same name are two chats
    let order = vec![room("jazz"), user("alice"), room("indie"), user("jazz")];
    let first = order.clone();
    db.write(move |conn| replace(conn, &first)).await.unwrap();
    assert_eq!(db.read(list).await.unwrap(), order);

    let moved = vec![user("jazz"), room("jazz"), room("indie")];
    let second = moved.clone();
    db.write(move |conn| replace(conn, &second)).await.unwrap();
    db.close().await;
    drop(db);

    let db = Database::open(&SCHEMA, &location).await.unwrap();
    assert_eq!(db.read(list).await.unwrap(), moved);
  }

  #[tokio::test]
  async fn a_database_from_before_pins_keeps_its_rooms_and_has_no_pins() {
    let dir = tempfile::tempdir().unwrap();
    let location = location(&dir);
    let before = Database::open(&BEFORE, &location).await.unwrap();
    before
      .write(|conn| conn.execute("INSERT INTO room_state (room_name) VALUES ('indie')", []))
      .await
      .unwrap();
    before.close().await;
    drop(before);

    let db = Database::open(&SCHEMA, &location).await.unwrap();
    assert_eq!(db.read(list).await.unwrap(), []);
    let rooms: i64 = db
      .read(|conn| conn.query_row("SELECT count(*) FROM room_state", [], |row| row.get(0)))
      .await
      .unwrap();
    assert_eq!(rooms, 1);
  }
}
