//! The messaging calls: conversations, rooms, their history and the message search.

use api_types::messaging::conversations::{
  CloseConversationQuery, Conversation, ConversationDraft, ConversationListQuery,
  ConversationMessagesQuery, ConversationQuery, PrivateMessage,
};
use api_types::messaging::rooms::{
  JoinRoom, JoinedRoom, ReadMarker, Room, RoomHistory, RoomHistoryList, RoomHistoryListQuery,
  RoomInfo, RoomMemberName, RoomMessagesQuery, RoomTicker, RoomUser, TickerText,
};
use api_types::messaging::{
  ChatPin, ChatPins, ExportFormat, ExportQuery, MessageSearchQuery, MessageSearchResults,
  MessageText, NotifyLevel, NotifySetting,
};
use bytes::Bytes;
use reqwest::StatusCode;

use super::{Client, Result, Route, bytes, done, json, send};

/// The route of one conversation.
fn conversation(username: &str) -> Route {
  Route::new("conversations").user(username)
}

/// The route of one joined room.
fn joined(room: &str) -> Route {
  Route::new("rooms/joined").room(room)
}

impl Client {
  /// `GET /conversations`: the conversations.
  pub async fn conversations(&self, query: &ConversationListQuery) -> Result<Vec<Conversation>> {
    json(self.get(Route::new("conversations").query(query)?)).await
  }

  /// `GET /conversations/{username}`: one conversation, with its latest messages when asked
  /// for.
  pub async fn conversation(&self, username: &str, include_messages: bool) -> Result<Conversation> {
    let route = conversation(username).query(&ConversationQuery { include_messages })?;
    json(self.get(route)).await
  }

  /// `GET /conversations/{username}/messages`: a page of one conversation's messages.
  pub async fn conversation_messages(
    &self,
    username: &str,
    query: &ConversationMessagesQuery,
  ) -> Result<Vec<PrivateMessage>> {
    json(self.get(conversation(username).then("messages").query(query)?)).await
  }

  /// `POST /conversations/{username}`: sends a private message.
  pub async fn send_private(&self, username: &str, message: &str) -> Result<()> {
    let body = MessageText(message.to_string());
    done(self.post(conversation(username)).json(&body)).await
  }

  /// `PUT /conversations/{username}`: acknowledges every message from a user.
  pub async fn acknowledge_all(&self, username: &str) -> Result<()> {
    done(self.put(conversation(username))).await
  }

  /// `PUT /conversations/{username}/{id}`: acknowledges one message from a user.
  pub async fn acknowledge(&self, username: &str, id: u32) -> Result<()> {
    done(self.put(conversation(username).segment(&id.to_string()))).await
  }

  /// `DELETE /conversations/{username}`: closes a conversation, and with `purge` deletes its
  /// messages.
  pub async fn close_conversation(&self, username: &str, purge: bool) -> Result<()> {
    let route = conversation(username).query(&CloseConversationQuery { purge })?;
    done(self.delete(route)).await
  }

  /// `GET /conversations/{username}/export`: a whole conversation as a file.
  pub async fn export_conversation(&self, username: &str, format: ExportFormat) -> Result<Bytes> {
    let route = conversation(username)
      .then("export")
      .query(&ExportQuery { format })?;
    bytes(self.get(route)).await
  }

  /// `PUT /conversations/{username}/notify`: when a conversation notifies.
  pub async fn set_conversation_notify(&self, username: &str, notify: NotifyLevel) -> Result<()> {
    let body = NotifySetting { notify };
    done(self.put(conversation(username).then("notify")).json(&body)).await
  }

  /// `PUT /conversations/{username}/draft`: keeps a conversation's unsent text.
  pub async fn set_draft(&self, username: &str, draft: &str) -> Result<()> {
    let body = ConversationDraft {
      draft: draft.to_string(),
    };
    done(self.put(conversation(username).then("draft")).json(&body)).await
  }

  /// `GET /rooms/joined`: the rooms we are in.
  pub async fn joined_rooms(&self) -> Result<Vec<JoinedRoom>> {
    json(self.get(Route::new("rooms/joined"))).await
  }

  /// `POST /rooms/joined`: joins a room; `None` when we were in it already.
  pub async fn join_room(&self, name: &str, private: bool) -> Result<Option<Room>> {
    let body = JoinRoom {
      name: name.to_string(),
      private,
    };
    let response = send(self.post(Route::new("rooms/joined")).json(&body)).await?;
    if response.status() == StatusCode::NOT_MODIFIED {
      return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&response.bytes().await?)?))
  }

  /// `GET /rooms/joined/{roomName}`: a room we are in.
  pub async fn room(&self, room: &str) -> Result<Room> {
    json(self.get(joined(room))).await
  }

  /// `DELETE /rooms/joined/{roomName}`: leaves a room.
  pub async fn leave_room(&self, room: &str) -> Result<()> {
    done(self.delete(joined(room))).await
  }

  /// `GET /rooms/joined/{roomName}/users`: the users in a room.
  pub async fn room_users(&self, room: &str) -> Result<Vec<RoomUser>> {
    json(self.get(joined(room).then("users"))).await
  }

  /// `GET /rooms/joined/{roomName}/messages`: a page of what was said in a room.
  pub async fn room_messages(&self, room: &str, query: &RoomMessagesQuery) -> Result<RoomHistory> {
    json(self.get(joined(room).then("messages").query(query)?)).await
  }

  /// `POST /rooms/joined/{roomName}/messages`: says something in a room.
  pub async fn say(&self, room: &str, message: &str) -> Result<()> {
    let body = MessageText(message.to_string());
    done(self.post(joined(room).then("messages")).json(&body)).await
  }

  /// `POST /rooms/joined/{roomName}/ticker`: sets our ticker in a room; empty clears it.
  pub async fn set_ticker(&self, room: &str, ticker: &str) -> Result<()> {
    let body = TickerText(ticker.to_string());
    done(self.post(joined(room).then("ticker")).json(&body)).await
  }

  /// `GET /rooms/joined/{roomName}/tickers`: the tickers in a room.
  pub async fn tickers(&self, room: &str) -> Result<Vec<RoomTicker>> {
    json(self.get(joined(room).then("tickers"))).await
  }

  /// `GET /chat/pins`: the pinned chats in the user's order, with their revision.
  pub async fn chat_pins(&self) -> Result<ChatPins> {
    json(self.get(Route::new("chat/pins"))).await
  }

  /// `PUT /chat/pins`: replaces the pinned chats, in the order given, and gives them with the
  /// revision the change made.
  pub async fn set_chat_pins(&self, pins: &[ChatPin]) -> Result<ChatPins> {
    json(self.put(Route::new("chat/pins")).json(pins)).await
  }

  /// `POST /rooms/joined/{roomName}/members`: adds a member to a private room.
  pub async fn add_room_member(&self, room: &str, username: &str) -> Result<()> {
    let body = RoomMemberName(username.to_string());
    done(self.post(joined(room).then("members")).json(&body)).await
  }

  /// `DELETE /rooms/joined/{roomName}/members/{username}`: takes a member out of a private
  /// room.
  pub async fn remove_room_member(&self, room: &str, username: &str) -> Result<()> {
    done(self.delete(joined(room).then("members").user(username))).await
  }

  /// `POST /rooms/joined/{roomName}/operators`: makes a member an operator.
  pub async fn add_room_operator(&self, room: &str, username: &str) -> Result<()> {
    let body = RoomMemberName(username.to_string());
    done(self.post(joined(room).then("operators")).json(&body)).await
  }

  /// `DELETE /rooms/joined/{roomName}/operators/{username}`: takes an operator back to member.
  pub async fn remove_room_operator(&self, room: &str, username: &str) -> Result<()> {
    done(self.delete(joined(room).then("operators").user(username))).await
  }

  /// `DELETE /rooms/private/{roomName}/membership`: gives up our membership of a private
  /// room.
  pub async fn drop_room_membership(&self, room: &str) -> Result<()> {
    done(self.delete(Route::new("rooms/private").room(room).then("membership"))).await
  }

  /// `DELETE /rooms/private/{roomName}/ownership`: gives up our ownership of a private room
  /// and leaves it.
  pub async fn drop_room_ownership(&self, room: &str) -> Result<()> {
    done(self.delete(Route::new("rooms/private").room(room).then("ownership"))).await
  }

  /// `PUT /rooms/joined/{roomName}/read`: marks a room read up to a message.
  pub async fn mark_room_read(&self, room: &str, local_id: i64) -> Result<()> {
    let body = ReadMarker { local_id };
    done(self.put(joined(room).then("read")).json(&body)).await
  }

  /// `PUT /rooms/joined/{roomName}/notify`: when a room notifies.
  pub async fn set_room_notify(&self, room: &str, notify: NotifyLevel) -> Result<()> {
    let body = NotifySetting { notify };
    done(self.put(joined(room).then("notify")).json(&body)).await
  }

  /// `GET /rooms/available`: the server's list of rooms.
  pub async fn available_rooms(&self) -> Result<Vec<RoomInfo>> {
    json(self.get(Route::new("rooms/available"))).await
  }

  /// `GET /rooms/history`: the rooms with stored messages.
  pub async fn room_history(&self, room: Option<&str>) -> Result<RoomHistoryList> {
    let query = RoomHistoryListQuery {
      room: room.map(str::to_string),
    };
    json(self.get(Route::new("rooms/history").query(&query)?)).await
  }

  /// `GET /rooms/history/{roomName}/messages`: a page of any room's stored messages.
  pub async fn stored_room_messages(
    &self,
    room: &str,
    query: &RoomMessagesQuery,
  ) -> Result<RoomHistory> {
    let route = Route::new("rooms/history").room(room).then("messages");
    json(self.get(route.query(query)?)).await
  }

  /// `DELETE /rooms/history/{roomName}`: deletes a room's stored messages.
  pub async fn delete_room_history(&self, room: &str) -> Result<()> {
    done(self.delete(Route::new("rooms/history").room(room))).await
  }

  /// `GET /rooms/history/{roomName}/export`: a room's whole stored history as a file.
  pub async fn export_room(&self, room: &str, format: ExportFormat) -> Result<Bytes> {
    let route = Route::new("rooms/history").room(room).then("export");
    bytes(self.get(route.query(&ExportQuery { format })?)).await
  }

  /// `GET /messages/search`: searches what was said.
  pub async fn search_messages(&self, query: &MessageSearchQuery) -> Result<MessageSearchResults> {
    json(self.get(Route::new("messages/search").query(query)?)).await
  }
}
