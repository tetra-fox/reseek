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
  /// A page of `room`'s stored history, with the gaps between its first message and its last,
  /// or now on the newest page.
  async fn history_page(
    &self,
    room: &str,
    query: RoomMessagesQuery,
  ) -> Result<RoomHistory, ApiError> {
    let room_name = room.to_string();
    let (messages, has_more, oldest) = self
      .db
      .read(move |conn| {
        let (messages, has_more) = history::page(conn, &room_name, &query)?;
        Ok((messages, has_more, history::oldest(conn, &room_name)?))
      })
      .await
      .map_err(internal)?;
    let gaps = match (oldest, messages.first()) {
      (Some(oldest), Some(first)) => {
        let to = match query.before {
          Some(_) => messages.last().map_or(first.timestamp, |m| m.timestamp),
          None => Utc::now(),
        };
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
