//! Changing a private room: adding and removing members and operators, and giving up our
//! membership or our ownership. The server says yes by announcing the change; a change it does
//! not announce in time was refused.

use soulseek_client::ClientError;
use tracing::debug;

use super::not_in;
use crate::api::error::ApiError;
use crate::messaging::Messaging;
use crate::messaging::conversations::unsent;

/// The answer to a change of a private room that did not go through.
fn refused(error: ClientError) -> ApiError {
  match error {
    ClientError::Timeout(_) => ApiError::forbidden("the server refused the change"),
    other => unsent(other),
  }
}

impl Messaging {
  /// Whether we belong to the private room `room` or are in it, else its 404.
  fn belongs(&self, room: &str) -> Result<(), ApiError> {
    let tracker = self.rooms.tracker();
    if tracker.get(room).is_some() || tracker.private(room).is_some() {
      Ok(())
    } else {
      Err(not_in(room))
    }
  }

  pub(in crate::messaging) async fn add_member(
    &self,
    room: &str,
    username: &str,
  ) -> Result<(), ApiError> {
    self.belongs(room)?;
    self
      .client()
      .add_private_room_member(room, username)
      .await
      .map_err(refused)?;
    debug!(room, username, "member added to a private room");
    Ok(())
  }

  pub(in crate::messaging) async fn remove_member(
    &self,
    room: &str,
    username: &str,
  ) -> Result<(), ApiError> {
    self.belongs(room)?;
    self
      .client()
      .remove_private_room_member(room, username)
      .await
      .map_err(refused)?;
    debug!(room, username, "member removed from a private room");
    Ok(())
  }

  pub(in crate::messaging) async fn add_operator(
    &self,
    room: &str,
    username: &str,
  ) -> Result<(), ApiError> {
    self.belongs(room)?;
    self
      .client()
      .add_private_room_moderator(room, username)
      .await
      .map_err(refused)?;
    debug!(room, username, "operator added to a private room");
    Ok(())
  }

  pub(in crate::messaging) async fn remove_operator(
    &self,
    room: &str,
    username: &str,
  ) -> Result<(), ApiError> {
    self.belongs(room)?;
    self
      .client()
      .remove_private_room_moderator(room, username)
      .await
      .map_err(refused)?;
    debug!(room, username, "operator removed from a private room");
    Ok(())
  }

  pub(in crate::messaging) async fn drop_membership(&self, room: &str) -> Result<(), ApiError> {
    self.belongs(room)?;
    self
      .client()
      .drop_private_room_membership(room)
      .await
      .map_err(refused)?;
    self.dropped(room).await;
    Ok(())
  }

  pub(in crate::messaging) async fn drop_ownership(&self, room: &str) -> Result<(), ApiError> {
    self.belongs(room)?;
    self
      .client()
      .drop_private_room_ownership(room)
      .await
      .map_err(refused)?;
    self.dropped(room).await;
    Ok(())
  }

  /// We no longer belong to the private room `room`.
  async fn dropped(&self, room: &str) {
    self.rooms.tracker().drop_private(room);
    self.rooms_changed();
    self.push_rooms().await;
  }
}
