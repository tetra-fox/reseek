//! Private rooms: adding and removing members and operators, and giving up membership or
//! ownership. Each completes when the server announces the change, except giving up ownership,
//! which the server does not answer.

use soulseek_proto::server::{
  PrivateRoomAddOperator, PrivateRoomAddUser, PrivateRoomDropMembership, PrivateRoomDropOwnership,
  PrivateRoomRemoveOperator, PrivateRoomRemoveUser,
};
use soulseek_proto::{ServerCode, ServerRequest};

use super::Client;
use super::request::{non_blank, room_key, room_user_key};
use crate::error::ClientError;

impl Client {
  /// adds `username` to our private room `room`; the user must accept invitations
  pub async fn add_private_room_member(
    &self,
    room: &str,
    username: &str,
  ) -> Result<(), ClientError> {
    non_blank("room name", room)?;
    non_blank("username", username)?;
    self
      .ask(
        &format!("add user {username} to private room {room}"),
        room_user_key(ServerCode::PrivateRoomAddUser, room, username),
        ServerRequest::PrivateRoomAddUser(PrivateRoomAddUser {
          room: room.to_owned(),
          username: username.to_owned(),
        }),
      )
      .await
  }

  /// makes `username` an operator of our private room `room`
  pub async fn add_private_room_moderator(
    &self,
    room: &str,
    username: &str,
  ) -> Result<(), ClientError> {
    non_blank("room name", room)?;
    non_blank("username", username)?;
    self
      .ask(
        &format!("add moderator {username} to private room {room}"),
        room_user_key(ServerCode::PrivateRoomAddOperator, room, username),
        ServerRequest::PrivateRoomAddOperator(PrivateRoomAddOperator {
          room: room.to_owned(),
          username: username.to_owned(),
        }),
      )
      .await
  }

  /// removes `username` from the private room `room`
  pub async fn remove_private_room_member(
    &self,
    room: &str,
    username: &str,
  ) -> Result<(), ClientError> {
    non_blank("room name", room)?;
    non_blank("username", username)?;
    self
      .ask(
        &format!("remove user {username} from private room {room}"),
        room_user_key(ServerCode::PrivateRoomRemoveUser, room, username),
        ServerRequest::PrivateRoomRemoveUser(PrivateRoomRemoveUser {
          room: room.to_owned(),
          username: username.to_owned(),
        }),
      )
      .await
  }

  /// takes operator rights in the private room `room` away from `username`
  pub async fn remove_private_room_moderator(
    &self,
    room: &str,
    username: &str,
  ) -> Result<(), ClientError> {
    non_blank("room name", room)?;
    non_blank("username", username)?;
    self
      .ask(
        &format!("remove moderator {username} from private room {room}"),
        room_user_key(ServerCode::PrivateRoomRemoveOperator, room, username),
        ServerRequest::PrivateRoomRemoveOperator(PrivateRoomRemoveOperator {
          room: room.to_owned(),
          username: username.to_owned(),
        }),
      )
      .await
  }

  /// leaves the private room `room` as a member
  pub async fn drop_private_room_membership(&self, room: &str) -> Result<(), ClientError> {
    non_blank("room name", room)?;
    self
      .ask(
        &format!("drop membership of private room {room}"),
        room_key(ServerCode::PrivateRoomRemoved, room),
        ServerRequest::PrivateRoomDropMembership(PrivateRoomDropMembership {
          room: room.to_owned(),
        }),
      )
      .await
  }

  /// gives up the private room `room` we own. The server turns it into a public room without
  /// an owner, keeps us in it and cuts its members off, telling no one, so this completes once
  /// the request is on the socket
  pub async fn drop_private_room_ownership(&self, room: &str) -> Result<(), ClientError> {
    non_blank("room name", room)?;
    self
      .tell(
        &format!("drop ownership of private room {room}"),
        ServerRequest::PrivateRoomDropOwnership(PrivateRoomDropOwnership {
          room: room.to_owned(),
        }),
      )
      .await
  }
}
