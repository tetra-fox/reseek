//! Changing a private room: adding and removing members and operators, and giving up our
//! membership or our ownership. The server says yes by announcing the change, and a change it
//! does not announce in time may still have been made. Giving up ownership it does not answer
//! at all.

use soulseek_client::ClientError;
use tracing::{debug, warn};

use super::not_in;
use crate::api::error::ApiError;
use crate::messaging::Messaging;
use crate::messaging::conversations::unsent;

/// The answer to a change of a private room that came to no confirmation; `action` names it in
/// the log.
fn unconfirmed(room: &str, action: &str, error: ClientError) -> ApiError {
  match error {
    ClientError::Timeout(_) => {
      warn!(room, action, error = %error, "private room change not confirmed in time");
      ApiError::gateway_timeout(
        "The server did not confirm the change in time; it may still have happened",
      )
      .logged()
    }
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
      .map_err(|e| unconfirmed(room, "add member", e))?;
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
      .map_err(|e| unconfirmed(room, "remove member", e))?;
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
      .map_err(|e| unconfirmed(room, "add operator", e))?;
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
      .map_err(|e| unconfirmed(room, "remove operator", e))?;
    debug!(room, username, "operator removed from a private room");
    Ok(())
  }

  pub(in crate::messaging) async fn drop_membership(&self, room: &str) -> Result<(), ApiError> {
    self.belongs(room)?;
    self
      .client()
      .drop_private_room_membership(room)
      .await
      .map_err(|e| unconfirmed(room, "drop membership", e))?;
    self.dropped(room).await;
    Ok(())
  }

  /// Gives up our private room `room` and leaves it. The server makes it a public room without
  /// an owner and keeps us in it, so the room is left to match what we show: not ours, not in.
  /// A room we know someone else owns is refused.
  pub(in crate::messaging) async fn drop_ownership(&self, room: &str) -> Result<(), ApiError> {
    self.belongs(room)?;
    let client = self.client();
    let us = client
      .own_username(&format!("give up private room {room}"))
      .map_err(unsent)?;
    let owner = self
      .rooms
      .tracker()
      .private(room)
      .and_then(|p| p.owner.clone());
    if owner.is_some_and(|owner| owner != us) {
      return Err(ApiError::forbidden(format!(
        "The room {room} is not ours to give up"
      )));
    }
    client
      .drop_private_room_ownership(room)
      .await
      .map_err(unsent)?;
    let joined = self.rooms.tracker().get(room).is_some_and(|r| r.joined);
    // ownership is given up either way; a leave that fails leaves the server with us in a public
    // room we no longer show, which the next join of it leaves and joins again
    if joined && let Err(e) = client.leave_room(room).await {
      warn!(room, error = %e, "room given up but not left");
    }
    self.dropped(room).await;
    Ok(())
  }

  /// We no longer belong to the private room `room`, so we are not in it, it is not joined
  /// again after a start, and the server's room list is asked for anew.
  pub(super) async fn dropped(&self, room: &str) {
    self.rooms.tracker().drop_private(room);
    self.rooms_changed();
    self.forget_room_list().await;
    self.note_left(room).await;
    self.push_rooms().await;
  }
}
