//! A room's stored history as the API reads it: pages of messages with the gaps in them, for a
//! room we are in or one we left, the rooms with stored messages, and deleting and exporting a
//! room's history.

use api_types::chat::ExportFormat;
use api_types::rooms::{RoomHistory, RoomHistoryList, RoomMessagesQuery};
use axum::response::Response;
use chrono::Utc;
use tracing::debug;

use super::{history, internal};
use crate::api::error::ApiError;
use crate::messaging::{Messaging, export, sessions};

impl Messaging {
  /// A page of `room`'s stored history, with the gaps from its first message to the message
  /// its `before` cursor names, which starts the next newer page, or to now on the newest page;
  /// so a gap between two pages is on the older one.
  async fn history_page(
    &self,
    room: &str,
    query: RoomMessagesQuery,
  ) -> Result<RoomHistory, ApiError> {
    let room_name = room.to_string();
    let (messages, has_more, oldest, newer) = self
      .db
      .read(move |conn| {
        let (messages, has_more) = history::page(conn, &room_name, &query)?;
        let newer = match query.before {
          Some(before) => history::arrived_from(conn, &room_name, before)?,
          None => None,
        };
        Ok((
          messages,
          has_more,
          history::oldest(conn, &room_name)?,
          newer,
        ))
      })
      .await
      .map_err(internal)?;
    let gaps = match (oldest, messages.first()) {
      (Some(oldest), Some(first)) => {
        let to = newer.unwrap_or_else(Utc::now);
        sessions::within(&self.history_gaps().await, oldest, first.timestamp, to)
      }
      _ => Vec::new(),
    };
    Ok(RoomHistory {
      messages,
      has_more,
      gaps,
    })
  }

  pub(in crate::messaging) async fn room_messages(
    &self,
    room: &str,
    query: RoomMessagesQuery,
  ) -> Result<RoomHistory, ApiError> {
    self.tracks(room)?;
    self.history_page(room, query).await
  }

  pub(in crate::messaging) async fn stored_messages(
    &self,
    room: &str,
    query: RoomMessagesQuery,
  ) -> Result<RoomHistory, ApiError> {
    let page = self.history_page(room, query).await?;
    if page.messages.is_empty() && query.before.is_none() && self.tracks(room).is_err() {
      return Err(ApiError::not_found(format!(
        "no messages of the room {room} are stored"
      )));
    }
    Ok(page)
  }

  pub(in crate::messaging) async fn room_history(
    &self,
    room: Option<String>,
  ) -> Result<RoomHistoryList, ApiError> {
    let joined = self.rooms.tracker().joined_names();
    let rooms = self
      .db
      .read(move |conn| {
        history::stored(conn, room.as_deref(), |name| {
          joined.iter().any(|joined| joined == name)
        })
      })
      .await
      .map_err(internal)?;
    Ok(RoomHistoryList { rooms })
  }

  pub(in crate::messaging) async fn delete_history(&self, room: &str) -> Result<(), ApiError> {
    let name = room.to_string();
    let deleted = self
      .db
      .write(move |conn| history::delete(conn, &name))
      .await
      .map_err(internal)?;
    if deleted == 0 {
      return Err(ApiError::not_found(format!(
        "no messages of the room {room} are stored"
      )));
    }
    debug!(room, deleted, "room history deleted");
    self.unread.changed();
    Ok(())
  }

  pub(in crate::messaging) async fn export_room(
    &self,
    room: &str,
    format: ExportFormat,
  ) -> Result<Response, ApiError> {
    let name = room.to_string();
    let messages = self
      .db
      .read(move |conn| history::all(conn, &name))
      .await
      .map_err(internal)?;
    if messages.is_empty() {
      return Err(ApiError::not_found(format!(
        "no messages of the room {room} are stored"
      )));
    }
    export::room(room, &messages, format)
  }
}

#[cfg(test)]
mod tests {
  use api_types::rooms::RoomMessagesQuery;
  use rusqlite::params;

  use super::history::{self, Arrived};
  use crate::testing::app;

  #[tokio::test]
  async fn a_gap_between_two_pages_is_on_the_older_one_only() {
    let app = app("", &[]).await;
    let messaging = &app.state.messaging;
    let ids = messaging
      .database()
      .write(|conn| {
        let mut ids = Vec::new();
        for (who, at) in [
          ("alice", "2026-10-26T10:00:00Z"),
          ("bob", "2026-10-26T12:00:00Z"),
        ] {
          let arrived = Arrived {
            room: "indie".into(),
            timestamp: at.parse().unwrap(),
            username: who.into(),
            message: "hello".into(),
            is_self: false,
            is_mention: false,
            hidden: false,
          };
          ids.push(history::insert(conn, &arrived)?.local_id);
        }
        // offline from 10:30 to 11:30, between the two messages
        conn.execute(
          "INSERT INTO sessions (logged_in_at, last_seen_at, ended_at, end_reason) \
           VALUES (?1, ?2, ?2, 'disconnected'), (?3, ?3, NULL, NULL)",
          params![
            "2026-10-26T09:00:00.000000Z",
            "2026-10-26T10:30:00.000000Z",
            "2026-10-26T11:30:00.000000Z"
          ],
        )?;
        Ok(ids)
      })
      .await
      .unwrap();
    let page = |before| RoomMessagesQuery {
      before,
      limit: 1,
      include_hidden: false,
    };
    let newest = messaging
      .stored_messages("indie", page(None))
      .await
      .unwrap();
    assert_eq!(newest.messages[0].local_id, ids[1]);
    assert!(newest.gaps.is_empty(), "{:?}", newest.gaps);
    let older = messaging
      .stored_messages("indie", page(Some(ids[1])))
      .await
      .unwrap();
    assert_eq!(older.messages[0].local_id, ids[0]);
    assert_eq!(
      older.gaps.len(),
      1,
      "the gap reaches the newer page's first message"
    );
    assert_eq!(
      older.gaps[0].to,
      Some("2026-10-26T11:30:00Z".parse().unwrap())
    );
  }
}
