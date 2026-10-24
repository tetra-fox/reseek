//! Chat rooms: joining and leaving, speaking, tickers, the room list and the public chat feed.

use std::sync::Arc;

use soulseek_proto::server::{
  JoinRoomRequest, LeaveRoom, RoomData, RoomList, SayInChatRoomRequest, SetRoomTicker,
};
use soulseek_proto::{RoomType, ServerCode, ServerRequest, validate_room_name};

use super::Client;
use super::request::{non_blank, non_empty, room_key};
use crate::error::ClientError;
use crate::waiter::WaitKey;

impl Client {
  /// Joins `room` and returns its members, with its owner and operators when it is private. A
  /// room that does not exist is created as `room_type`. The name is checked first, since the
  /// server answers a bad name only with a private message from `server`. Joining a name a
  /// private room we are not in holds fails with `RoomJoinForbidden`; no answer at all means we
  /// are in the room already, which fails with `NoResponse`.
  pub async fn join_room(&self, room: &str, room_type: RoomType) -> Result<RoomData, ClientError> {
    non_blank("room name", room)?;
    validate_room_name(room)
      .map_err(|_| ClientError::InvalidArgument("Invalid room name".into()))?;
    let joined = self
      .ask(
        &format!("join chat room {room}"),
        room_key(ServerCode::JoinRoom, room),
        ServerRequest::JoinRoom(JoinRoomRequest {
          room: room.to_owned(),
          room_type,
        }),
      )
      .await;
    match joined {
      Err(ClientError::Timeout(_)) => Err(ClientError::NoResponse(format!(
        "The server didn't respond to the request to join chat room {room}. This probably indicates that the room is already joined."
      ))),
      joined => joined,
    }
  }

  /// Leaves `room`. No answer means we were not in it, which fails with `NoResponse`.
  pub async fn leave_room(&self, room: &str) -> Result<(), ClientError> {
    non_blank("room name", room)?;
    let left = self
      .ask(
        &format!("leave chat room {room}"),
        room_key(ServerCode::LeaveRoom, room),
        ServerRequest::LeaveRoom(LeaveRoom {
          room: room.to_owned(),
        }),
      )
      .await;
    match left {
      Err(ClientError::Timeout(_)) => Err(ClientError::NoResponse(format!(
        "The server didn't respond to the request to leave chat room {room}. This probably indicates that the room is not joined."
      ))),
      left => left,
    }
  }

  /// says `message` in `room`; the server echoes it back to us like any other line
  pub async fn send_room_message(&self, room: &str, message: &str) -> Result<(), ClientError> {
    non_blank("room name", room)?;
    non_empty("message", message)?;
    self
      .tell(
        &format!("send message to room {room}"),
        ServerRequest::SayInChatRoom(SayInChatRoomRequest {
          room: room.to_owned(),
          message: message.to_owned(),
        }),
      )
      .await
  }

  /// sets our ticker in `room`
  pub async fn set_room_ticker(&self, room: &str, message: &str) -> Result<(), ClientError> {
    non_blank("room name", room)?;
    non_empty("message", message)?;
    self
      .tell(
        &format!("set ticker for room {room}"),
        ServerRequest::SetRoomTicker(SetRoomTicker {
          room: room.to_owned(),
          ticker: message.to_owned(),
        }),
      )
      .await
  }

  /// every room on the server with its member count
  pub async fn get_room_list(&self) -> Result<Arc<RoomList>, ClientError> {
    self
      .ask(
        "get the room list",
        WaitKey::Server(ServerCode::RoomList),
        ServerRequest::RoomList,
      )
      .await
  }

  /// starts the feed of every line said in every public room
  pub async fn start_public_chat(&self) -> Result<(), ClientError> {
    self
      .tell("start public chat", ServerRequest::AskPublicChat)
      .await
  }

  /// stops the public chat feed
  pub async fn stop_public_chat(&self) -> Result<(), ClientError> {
    self
      .tell("stop public chat", ServerRequest::StopPublicChat)
      .await
  }
}
