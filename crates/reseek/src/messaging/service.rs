//! The messaging calls of the API, each answered by the engine's own method of the same name.

use api_types::chat::{ExportFormat, MessageSearchQuery, MessageSearchResults, NotifyLevel};
use api_types::conversations::{
  Conversation, ConversationListQuery, ConversationMessagesQuery, PrivateMessage,
};
use api_types::rooms::{
  JoinRoom, JoinedRoom, Room, RoomHistory, RoomHistoryList, RoomInfo, RoomMessagesQuery,
  RoomTicker, RoomUser,
};
use async_trait::async_trait;
use axum::response::Response;

use super::Messaging;
use crate::api::error::ApiError;
use crate::services::{Joined, MessagingService};

#[async_trait]
impl MessagingService for Messaging {
  async fn conversations(
    &self,
    query: ConversationListQuery,
  ) -> Result<Vec<Conversation>, ApiError> {
    Messaging::conversations(self, query).await
  }

  async fn conversation(
    &self,
    username: &str,
    include_messages: bool,
  ) -> Result<Conversation, ApiError> {
    Messaging::conversation(self, username, include_messages).await
  }

  async fn conversation_messages(
    &self,
    username: &str,
    query: ConversationMessagesQuery,
  ) -> Result<Vec<PrivateMessage>, ApiError> {
    Messaging::conversation_messages(self, username, query).await
  }

  async fn send_private(&self, username: &str, message: String) -> Result<(), ApiError> {
    Messaging::send_private(self, username, message).await
  }

  async fn acknowledge_all(&self, username: &str) -> Result<(), ApiError> {
    Messaging::acknowledge_all(self, username).await
  }

  async fn acknowledge(&self, username: &str, id: u32) -> Result<(), ApiError> {
    Messaging::acknowledge(self, username, id).await
  }

  async fn close(&self, username: &str, purge: bool) -> Result<(), ApiError> {
    Messaging::close(self, username, purge).await
  }

  async fn export_conversation(
    &self,
    username: &str,
    format: ExportFormat,
  ) -> Result<Response, ApiError> {
    Messaging::export_conversation(self, username, format).await
  }

  async fn set_conversation_notify(
    &self,
    username: &str,
    notify: NotifyLevel,
  ) -> Result<(), ApiError> {
    Messaging::set_conversation_notify(self, username, notify).await
  }

  async fn set_draft(&self, username: &str, draft: String) -> Result<(), ApiError> {
    Messaging::set_draft(self, username, draft).await
  }

  async fn joined_rooms(&self) -> Result<Vec<JoinedRoom>, ApiError> {
    Messaging::joined_rooms(self).await
  }

  async fn room(&self, room: &str) -> Result<Room, ApiError> {
    Messaging::room(self, room).await
  }

  async fn room_users(&self, room: &str) -> Result<Vec<RoomUser>, ApiError> {
    Messaging::room_users(self, room)
  }

  async fn room_messages(
    &self,
    room: &str,
    query: RoomMessagesQuery,
  ) -> Result<RoomHistory, ApiError> {
    Messaging::room_messages(self, room, query).await
  }

  async fn join(&self, request: JoinRoom) -> Result<Joined, ApiError> {
    Messaging::join(self, request).await
  }

  async fn leave(&self, room: &str) -> Result<(), ApiError> {
    Messaging::leave(self, room).await
  }

  async fn send_room(&self, room: &str, message: String) -> Result<(), ApiError> {
    Messaging::send_room(self, room, message).await
  }

  async fn set_ticker(&self, room: &str, ticker: String) -> Result<(), ApiError> {
    Messaging::set_ticker(self, room, ticker).await
  }

  async fn tickers(&self, room: &str) -> Result<Vec<RoomTicker>, ApiError> {
    Messaging::tickers(self, room)
  }

  async fn add_member(&self, room: &str, username: &str) -> Result<(), ApiError> {
    Messaging::add_member(self, room, username).await
  }

  async fn remove_member(&self, room: &str, username: &str) -> Result<(), ApiError> {
    Messaging::remove_member(self, room, username).await
  }

  async fn add_operator(&self, room: &str, username: &str) -> Result<(), ApiError> {
    Messaging::add_operator(self, room, username).await
  }

  async fn remove_operator(&self, room: &str, username: &str) -> Result<(), ApiError> {
    Messaging::remove_operator(self, room, username).await
  }

  async fn drop_membership(&self, room: &str) -> Result<(), ApiError> {
    Messaging::drop_membership(self, room).await
  }

  async fn drop_ownership(&self, room: &str) -> Result<(), ApiError> {
    Messaging::drop_ownership(self, room).await
  }

  async fn available_rooms(&self) -> Result<Vec<RoomInfo>, ApiError> {
    Messaging::available_rooms(self).await
  }

  async fn mark_read(&self, room: &str, local_id: i64) -> Result<(), ApiError> {
    Messaging::mark_read(self, room, local_id).await
  }

  async fn set_room_notify(&self, room: &str, notify: NotifyLevel) -> Result<(), ApiError> {
    Messaging::set_room_notify(self, room, notify).await
  }

  async fn room_history(&self, room: Option<String>) -> Result<RoomHistoryList, ApiError> {
    Messaging::room_history(self, room).await
  }

  async fn stored_messages(
    &self,
    room: &str,
    query: RoomMessagesQuery,
  ) -> Result<RoomHistory, ApiError> {
    Messaging::stored_messages(self, room, query).await
  }

  async fn delete_history(&self, room: &str) -> Result<(), ApiError> {
    Messaging::delete_history(self, room).await
  }

  async fn export_room(&self, room: &str, format: ExportFormat) -> Result<Response, ApiError> {
    Messaging::export_room(self, room, format).await
  }

  async fn search(&self, query: MessageSearchQuery) -> Result<MessageSearchResults, ApiError> {
    Messaging::search(self, query).await
  }
}
