//! Private conversations and rooms: what is said, joining and leaving, and the stored history.

use api_types::messaging::conversations::{
  Conversation, ConversationListQuery, ConversationMessagesQuery, PrivateMessage,
};
use api_types::messaging::rooms::{
  JoinRoom, JoinedRoom, Room, RoomHistory, RoomHistoryList, RoomInfo, RoomMessagesQuery,
  RoomTicker, RoomUser,
};
use api_types::messaging::{
  ChatPin, ChatPins, ExportFormat, MessageSearchQuery, MessageSearchResults, NotifyLevel,
};
use api_types::state::OwnStatus;
use async_trait::async_trait;
use axum::response::Response;

use super::not_yet;
use crate::api::error::ApiError;

const DOMAIN: &str = "messaging";

/// What asking to join a room did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Joined {
  /// the room was joined, or created, now
  New(Room),
  /// we were in the room already
  Already,
}

/// Conversations with other users and the rooms we are in. A call naming a room we are not in
/// answers 404; one that needs the server while we are not logged in answers 409.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait MessagingService: Send + Sync {
  /// The conversations, without their messages, by username.
  async fn conversations(
    &self,
    query: ConversationListQuery,
  ) -> Result<Vec<Conversation>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// One conversation, active or not, with its latest messages when asked for.
  async fn conversation(
    &self,
    username: &str,
    include_messages: bool,
  ) -> Result<Conversation, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A page of one conversation's messages, oldest first.
  async fn conversation_messages(
    &self,
    username: &str,
    query: ConversationMessagesQuery,
  ) -> Result<Vec<PrivateMessage>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Sends a private message on one line, line breaks turned into spaces, and records it as
  /// sent. A blocked user is refused with 403.
  async fn send_private(&self, username: &str, message: String) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Acknowledges every unacknowledged message from a user to the server.
  async fn acknowledge_all(&self, username: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Acknowledges one message from a user to the server.
  async fn acknowledge(&self, username: &str, id: u32) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Acknowledges a conversation and hides it until the next message; with `purge` also
  /// deletes its messages.
  async fn close(&self, username: &str, purge: bool) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A whole conversation as a file to save, as the whole response.
  async fn export_conversation(
    &self,
    username: &str,
    format: ExportFormat,
  ) -> Result<Response, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// When a conversation notifies.
  async fn set_conversation_notify(
    &self,
    username: &str,
    notify: NotifyLevel,
  ) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Keeps the unsent text of a conversation.
  async fn set_draft(&self, username: &str, draft: String) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The pinned chats in the user's order, with their revision.
  async fn pins(&self) -> Result<ChatPins, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Makes `pins` the pinned chats, in that order, tells open streams, and gives the list with
  /// the revision the change made.
  async fn set_pins(&self, pins: Vec<ChatPin>) -> Result<ChatPins, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The rooms we are in.
  async fn joined_rooms(&self) -> Result<Vec<JoinedRoom>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A room we are in, with its users and tickers.
  async fn room(&self, room: &str) -> Result<Room, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The users in a room we are in.
  async fn room_users(&self, room: &str) -> Result<Vec<RoomUser>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A page of a joined room's stored messages, with the gaps in it.
  async fn room_messages(
    &self,
    room: &str,
    query: RoomMessagesQuery,
  ) -> Result<RoomHistory, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Joins a room, creating it when it does not exist. The name has passed the API's checks;
  /// a private room someone else holds is refused with 403.
  async fn join(&self, request: JoinRoom) -> Result<Joined, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Leaves a room.
  async fn leave(&self, room: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Says something in a room. The message arrives back from the server like anyone's.
  async fn send_room(&self, room: &str, message: String) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Sets our ticker in a room; an empty one clears it.
  async fn set_ticker(&self, room: &str, ticker: String) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The tickers in a room.
  async fn tickers(&self, room: &str) -> Result<Vec<RoomTicker>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Adds a member to a private room we own or operate. The server refusing is a 403.
  async fn add_member(&self, room: &str, username: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Takes a member out of a private room. The server refusing is a 403.
  async fn remove_member(&self, room: &str, username: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Makes a member of a private room we own an operator. The server refusing is a 403.
  async fn add_operator(&self, room: &str, username: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Takes a private room's operator back to member. The server refusing is a 403.
  async fn remove_operator(&self, room: &str, username: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Gives up our membership of a private room.
  async fn drop_membership(&self, room: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Gives up our ownership of a private room and leaves it.
  async fn drop_ownership(&self, room: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The server's list of rooms, asked of it now.
  async fn available_rooms(&self) -> Result<Vec<RoomInfo>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Marks a room read up to the message with `local_id`.
  async fn mark_read(&self, room: &str, local_id: i64) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// When a room notifies.
  async fn set_room_notify(&self, room: &str, notify: NotifyLevel) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The rooms with stored messages, one of them when `room` names it, the rooms we left
  /// included.
  async fn room_history(&self, room: Option<String>) -> Result<RoomHistoryList, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A page of any room's stored messages, a room we left included, with the gaps in it.
  async fn stored_messages(
    &self,
    room: &str,
    query: RoomMessagesQuery,
  ) -> Result<RoomHistory, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Deletes a room's stored messages.
  async fn delete_history(&self, room: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A room's whole stored history as a file to save, as the whole response.
  async fn export_room(&self, room: &str, format: ExportFormat) -> Result<Response, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Searches the stored messages of conversations and rooms.
  async fn search(&self, query: MessageSearchQuery) -> Result<MessageSearchResults, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Sets us online or away by hand, as `by` asked; the server is told at once while logged in,
  /// else at the next login.
  async fn set_status(&self, status: OwnStatus, by: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }
}
