//! The chats pinned above the others in the chat list, rooms and users in one order of the
//! user's. A pin is a setting of the chat, so it stays while the room is left or the conversation
//! closed, the list then leaving it out until the chat is back.

use api_types::live::LiveFrame;
use api_types::messaging::{ChatKind, ChatPin, ChatPins};
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

/// The pinned chats in the user's order, with their revision.
fn list(conn: &Connection) -> rusqlite::Result<ChatPins> {
  // one read transaction, so the revision is the one of the list read with it
  let tx = conn.unchecked_transaction()?;
  let revision = tx.query_row("SELECT revision FROM chat_pins_revision", [], |row| {
    row.get(0)
  })?;
  let pins = tx
    .prepare_cached("SELECT kind, name FROM chat_pins ORDER BY position")?
    .query_map([], |row| {
      Ok(ChatPin {
        kind: chat_kind(row.get_ref(0)?.as_str()?)?,
        name: row.get(1)?,
      })
    })?
    .collect::<rusqlite::Result<_>>()?;
  tx.commit()?;
  Ok(ChatPins { revision, pins })
}

/// Makes `pins`, each named once, the pinned chats in that order, and gives the revision the
/// change made. Revisions follow the order the writes commit in.
fn replace(conn: &mut Connection, pins: &[ChatPin]) -> rusqlite::Result<i64> {
  let tx = conn.transaction()?;
  tx.execute("DELETE FROM chat_pins", [])?;
  {
    let mut insert =
      tx.prepare_cached("INSERT INTO chat_pins (kind, name, position) VALUES (?1, ?2, ?3)")?;
    for (position, pin) in (0_i64..).zip(pins) {
      insert.execute(params![kind_text(pin.kind), pin.name, position])?;
    }
  }
  let revision = tx.query_row(
    "UPDATE chat_pins_revision SET revision = revision + 1 RETURNING revision",
    [],
    |row| row.get(0),
  )?;
  tx.commit()?;
  Ok(revision)
}

impl Messaging {
  pub(super) async fn pins(&self) -> Result<ChatPins, ApiError> {
    self.db.read(list).await.map_err(ApiError::internal)
  }

  pub(super) async fn set_pins(&self, pins: Vec<ChatPin>) -> Result<ChatPins, ApiError> {
    let set = self
      .db
      .write(move |conn| {
        let revision = replace(conn, &pins)?;
        Ok(ChatPins { revision, pins })
      })
      .await
      .map_err(ApiError::internal)?;
    debug!(
      pins = set.pins.len(),
      revision = set.revision,
      "chat pins set"
    );
    // the frame and the answer each carry the list
    self.live.publish(LiveFrame::ChatPins(set.clone()));
    Ok(set)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::{Database, Location};
  use crate::messaging::SCHEMA;
  use crate::testing::{app, request};

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
    let location = Location::on_disk(dir.path());
    let db = Database::open(&SCHEMA, &location).await.unwrap();
    // a room and a user of the same name are two chats
    let order = vec![room("jazz"), user("alice"), room("indie"), user("jazz")];
    let first = order.clone();
    db.write(move |conn| replace(conn, &first)).await.unwrap();
    assert_eq!(db.read(list).await.unwrap().pins, order);

    let moved = vec![user("jazz"), room("jazz"), room("indie")];
    let second = moved.clone();
    db.write(move |conn| replace(conn, &second)).await.unwrap();
    db.close().await;
    drop(db);

    let db = Database::open(&SCHEMA, &location).await.unwrap();
    assert_eq!(
      db.read(list).await.unwrap(),
      ChatPins {
        revision: 2,
        pins: moved
      },
      "the revision is kept with the list"
    );
  }

  #[tokio::test]
  async fn the_newest_revision_streamed_is_the_list_written_last() {
    let config = "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n";
    let app = app(config, &[]).await;
    let mut stream = app
      .stream_from(
        "127.0.0.1:40000",
        request("GET", "/api/v0/live?topics=rooms", None),
      )
      .await;
    stream.until("hello").await;
    let snapshot = stream.until("chat.pins").await;
    assert_eq!(snapshot.last().unwrap().data["revision"], 0);

    let messaging = &app.state.messaging;
    let (first, second) = tokio::join!(
      messaging.set_pins(vec![room("jazz")]),
      messaging.set_pins(vec![user("alice"), room("indie")]),
    );
    let mut heard = Vec::new();
    for _ in 0..2 {
      let frame = stream.until("chat.pins").await.pop().unwrap();
      heard.push(serde_json::from_value::<ChatPins>(frame.data).unwrap());
    }
    // the frames may come in either order, each with the revision its write committed with
    heard.sort_by_key(|pins| pins.revision);
    let mut answered = vec![first.unwrap(), second.unwrap()];
    answered.sort_by_key(|pins| pins.revision);
    assert_eq!(heard, answered);
    assert_eq!(
      heard.iter().map(|pins| pins.revision).collect::<Vec<_>>(),
      [1, 2]
    );
    assert_eq!(
      messaging.pins().await.unwrap(),
      heard[1],
      "the highest revision is what the database holds"
    );
  }
}
