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
  /// its `before` cursor names, which starts the next newer page, so a gap between two pages is
  /// on the older one. The newest page's gaps run to now while we are in the room, and once we
  /// left it to when we left, or to its last message when that is not noted.
  async fn history_page(
    &self,
    room: &str,
    query: RoomMessagesQuery,
  ) -> Result<RoomHistory, ApiError> {
    let room_name = room.to_string();
    let present = self.tracks(room).is_ok();
    let (messages, has_more, oldest, end) = self
      .db
      .read(move |conn| {
        let (messages, has_more) = history::page(conn, &room_name, &query)?;
        let end = match query.before {
          Some(before) => history::arrived_from(conn, &room_name, before)?,
          None if present => None,
          None => match history::left_at(conn, &room_name)? {
            Some(left) => Some(left),
            None => history::newest(conn, &room_name)?,
          },
        };
        Ok((messages, has_more, history::oldest(conn, &room_name)?, end))
      })
      .await
      .map_err(internal)?;
    let gaps = match (oldest, messages.first()) {
      (Some(oldest), Some(first)) => {
        let to = end.unwrap_or_else(Utc::now);
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
  use api_types::rooms::{RoomHistory, RoomMessagesQuery};
  use rusqlite::params;
  use soulseek_proto::server::RoomData;

  use super::history::{self, Arrived};
  use crate::messaging::Messaging;
  use crate::testing::app;

  /// Stores a message in `indie` at each of `times`, and one session that ended, disconnected,
  /// at `ended` and the next, still running, from `again`; returns the messages' local ids.
  async fn stored(
    messaging: &Messaging,
    times: &[&'static str],
    ended: &str,
    again: &str,
  ) -> Vec<i64> {
    let (times, ended, again) = (times.to_vec(), ended.to_string(), again.to_string());
    messaging
      .database()
      .write(move |conn| {
        let mut ids = Vec::new();
        for at in times {
          let arrived = Arrived {
            room: "indie".into(),
            timestamp: at.parse().unwrap(),
            username: "alice".into(),
            message: "hello".into(),
            is_self: false,
            is_mention: false,
            hidden: false,
          };
          ids.push(history::insert(conn, &arrived)?.local_id);
        }
        conn.execute(
          "INSERT INTO sessions (logged_in_at, last_seen_at, ended_at, end_reason) \
           VALUES ('2026-10-26T09:00:00.000000Z', ?1, ?1, 'disconnected'), (?2, ?2, NULL, NULL)",
          params![ended, again],
        )?;
        Ok(ids)
      })
      .await
      .unwrap()
  }

  /// One message of `indie`'s history before `before`, or the newest.
  async fn page(messaging: &Messaging, before: Option<i64>) -> RoomHistory {
    let query = RoomMessagesQuery {
      before,
      limit: 1,
      include_hidden: false,
    };
    messaging.stored_messages("indie", query).await.unwrap()
  }

  #[tokio::test]
  async fn a_gap_between_two_pages_is_on_the_older_one_only() {
    let app = app("", &[]).await;
    let messaging = &app.state.messaging;
    // offline from 10:30 to 11:30, between the two messages
    let ids = stored(
      messaging,
      &["2026-10-26T10:00:00Z", "2026-10-26T12:00:00Z"],
      "2026-10-26T10:30:00.000000Z",
      "2026-10-26T11:30:00.000000Z",
    )
    .await;
    let newest = page(messaging, None).await;
    assert_eq!(newest.messages[0].local_id, ids[1]);
    assert!(newest.gaps.is_empty(), "{:?}", newest.gaps);
    let older = page(messaging, Some(ids[1])).await;
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

  #[tokio::test]
  async fn a_room_we_left_shows_no_gap_after_its_last_message() {
    let app = app("", &[]).await;
    let messaging = &app.state.messaging;
    // the room is not one we are in; we were offline from 13:00 to 14:00, after its messages
    stored(
      messaging,
      &["2026-10-26T10:00:00Z", "2026-10-26T12:00:00Z"],
      "2026-10-26T13:00:00.000000Z",
      "2026-10-26T14:00:00.000000Z",
    )
    .await;
    let newest = page(messaging, None).await;
    assert!(newest.gaps.is_empty(), "{:?}", newest.gaps);
  }

  #[tokio::test]
  async fn a_room_we_are_still_in_shows_the_gap_to_now() {
    let app = app("", &[]).await;
    let messaging = &app.state.messaging;
    stored(
      messaging,
      &["2026-10-26T10:00:00Z", "2026-10-26T12:00:00Z"],
      "2026-10-26T13:00:00.000000Z",
      "2026-10-26T14:00:00.000000Z",
    )
    .await;
    // in the room, then the connection lost: the room is kept to be joined again
    let data = RoomData {
      name: "indie".into(),
      users: Vec::new(),
      private: None,
    };
    messaging.rooms.tracker().joined(&data, "me");
    messaging.rooms.tracker().disconnected();
    let newest = page(messaging, None).await;
    assert_eq!(newest.gaps.len(), 1, "{:?}", newest.gaps);
    assert_eq!(
      newest.gaps[0].from,
      "2026-10-26T13:00:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap()
    );
  }

  #[tokio::test]
  async fn a_room_we_left_shows_the_gaps_until_we_left() {
    let app = app("", &[]).await;
    let messaging = &app.state.messaging;
    // the last message at 18:00, offline from 20:00 to 21:00, and left at 22:00
    stored(
      messaging,
      &["2026-10-26T17:00:00Z", "2026-10-26T18:00:00Z"],
      "2026-10-26T20:00:00.000000Z",
      "2026-10-26T21:00:00.000000Z",
    )
    .await;
    let left = "2026-10-26T22:00:00Z".parse().unwrap();
    messaging
      .database()
      .write(move |conn| history::left(conn, "indie", left))
      .await
      .unwrap();
    let newest = page(messaging, None).await;
    assert_eq!(newest.gaps.len(), 1, "{:?}", newest.gaps);
    assert_eq!(
      newest.gaps[0].to,
      Some("2026-10-26T21:00:00Z".parse().unwrap())
    );
  }
}
