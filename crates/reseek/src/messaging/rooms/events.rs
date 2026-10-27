//! What the server tells us about rooms: who joins and leaves, what is said, the tickers, and
//! the private rooms we belong to. A message is stored before anyone is told of it; one for a
//! room we are not in is dropped, and one from a user blocked for rooms is stored hidden and
//! told to no one.

use std::collections::BTreeMap;

use api_types::blocking::BlockScope;
use api_types::events::{Event as BusEvent, RoomMessageReceivedEvent};
use api_types::live::LiveFrame;
use api_types::rooms::{RoomMessageReceived, RoomUserJoined, RoomUserLeft};
use chrono::Utc;
use soulseek_client::{Event, RoomMembers};
use soulseek_proto::server::UserData;
use tracing::{debug, info, trace, warn};

use super::history::{self, Arrived};
use super::tracker::{country_of, room_user};
use crate::messaging::{Messaging, mentions};
use crate::users::Touch;

/// The room a client event is about, when it is about one.
pub fn room_of(event: &Event) -> Option<&str> {
  match event {
    Event::RoomJoined { room_name, .. }
    | Event::RoomLeft { room_name, .. }
    | Event::RoomMessageReceived { room_name, .. }
    | Event::RoomTickerListReceived { room_name, .. }
    | Event::RoomTickerAdded { room_name, .. }
    | Event::RoomTickerRemoved { room_name, .. }
    | Event::PrivateRoomUserAdded { room_name, .. }
    | Event::PrivateRoomUserRemoved { room_name, .. }
    | Event::PrivateRoomOperatorAdded { room_name, .. }
    | Event::PrivateRoomOperatorRemoved { room_name, .. }
    | Event::PrivateRoomUserListReceived(RoomMembers { room_name, .. })
    | Event::PrivateRoomModeratedUserListReceived(RoomMembers { room_name, .. })
    | Event::PrivateRoomMembershipAdded(room_name)
    | Event::PrivateRoomMembershipRemoved(room_name)
    | Event::PrivateRoomModerationAdded(room_name)
    | Event::PrivateRoomModerationRemoved(room_name) => Some(room_name),
    _ => None,
  }
}

impl Messaging {
  /// Handles a client event about a room.
  pub(in crate::messaging) async fn room_event(&self, event: Event) {
    match event {
      Event::RoomJoined {
        room_name,
        username,
        user_data,
      } => self.user_joined(room_name, username, &user_data).await,
      Event::RoomLeft {
        room_name,
        username,
      } => self.user_left(room_name, username),
      Event::RoomMessageReceived {
        room_name,
        username,
        message,
      } => self.room_message(room_name, username, message).await,
      Event::RoomTickerListReceived { room_name, tickers } => {
        self.change_tickers(&room_name, |all| {
          *all = tickers
            .into_iter()
            .map(|ticker| (ticker.username, ticker.message))
            .collect();
        });
      }
      Event::RoomTickerAdded { room_name, ticker } => self.change_tickers(&room_name, |all| {
        all.insert(ticker.username, ticker.message);
      }),
      Event::RoomTickerRemoved {
        room_name,
        username,
      } => self.change_tickers(&room_name, |all| {
        all.remove(&username);
      }),
      Event::PrivateRoomUserListReceived(RoomMembers { room_name, users }) => {
        self.change_private(&room_name, |room| {
          room.members = users.into_iter().collect()
        });
      }
      Event::PrivateRoomModeratedUserListReceived(RoomMembers { room_name, users }) => {
        self.change_private(&room_name, |room| {
          room.operators = users.into_iter().collect()
        });
      }
      Event::PrivateRoomUserAdded {
        room_name,
        username,
      } => self.change_private(&room_name, |room| {
        room.members.insert(username);
      }),
      Event::PrivateRoomUserRemoved {
        room_name,
        username,
      } => self.change_private(&room_name, |room| {
        room.members.remove(&username);
        room.operators.remove(&username);
      }),
      Event::PrivateRoomOperatorAdded {
        room_name,
        username,
      } => self.change_private(&room_name, |room| {
        room.operators.insert(username);
      }),
      Event::PrivateRoomOperatorRemoved {
        room_name,
        username,
      } => self.change_private(&room_name, |room| {
        room.operators.remove(&username);
      }),
      Event::PrivateRoomMembershipAdded(room) => {
        info!(room, "Added to private room");
        self.rooms.tracker().change_private(&room, |_| {});
        self.push_rooms().await;
      }
      Event::PrivateRoomMembershipRemoved(room) => {
        info!(room, "Removed from private room");
        self.rooms.tracker().drop_private(&room);
        self.rooms_changed();
        self.push_rooms().await;
      }
      Event::PrivateRoomModerationAdded(room) => {
        info!(room, "Promoted to moderator in private room");
        if let Some(us) = self.us() {
          self.change_private(&room, |private| {
            private.operators.insert(us);
          });
        }
      }
      Event::PrivateRoomModerationRemoved(room) => {
        info!(room, "Demoted from moderator in private room");
        if let Some(us) = self.us() {
          self.change_private(&room, |private| {
            private.operators.remove(&us);
          });
        }
      }
      other => trace!(?other, "client event no room engine follows"),
    }
  }

  /// Pushes the room `name` as it is now, when it is tracked.
  fn push_room(&self, name: &str) {
    let room = self.rooms.tracker().room(name);
    if let Some(room) = room {
      self.live.publish(LiveFrame::RoomsUpdate(Box::new(room)));
    }
  }

  fn change_tickers(&self, room: &str, change: impl FnOnce(&mut BTreeMap<String, String>)) {
    if self.rooms.tracker().tickers(room, change) {
      self.push_room(room);
    } else {
      debug!(room, "tickers of a room we are not in dropped");
    }
  }

  fn change_private(&self, room: &str, change: impl FnOnce(&mut super::tracker::PrivateRoom)) {
    self.rooms.tracker().change_private(room, change);
    self.push_room(room);
  }

  /// `username` joined `room`: ourselves, which the server confirms after our join, or another
  /// user, who is added to the room's members.
  async fn user_joined(&self, room: String, username: String, user: &UserData) {
    let us = self.us_logged_in();
    if username == us {
      if self.rooms.tracker().confirmed(&room) {
        debug!(room, "Joined room");
        self.rooms_changed();
      }
      return;
    }
    let member = room_user(user, &us);
    if !self.rooms.tracker().user_joined(&room, member.clone()) {
      return debug!(room, "a join to a room we are not in dropped");
    }
    trace!(room, username, "user joined a room");
    if let Some(country) = country_of(user) {
      self
        .users
        .countries_seen(vec![(username, country.to_string())])
        .await;
    }
    self
      .live
      .publish(LiveFrame::RoomsUserJoined(RoomUserJoined {
        room_name: room,
        user: member,
      }));
  }

  /// `username` left `room`. Our own leave is the one we asked for, which has taken the room
  /// off already.
  fn user_left(&self, room: String, username: String) {
    if self.us().as_deref() == Some(username.as_str()) {
      return debug!(room, "Left room");
    }
    if !self.rooms.tracker().user_left(&room, &username) {
      return debug!(room, "a leave from a room we are not in dropped");
    }
    trace!(room, username, "user left a room");
    self.live.publish(LiveFrame::RoomsUserLeft(RoomUserLeft {
      room_name: room,
      username,
    }));
  }

  /// A message said in `room`, ours included: stored first, then raised and pushed; a message
  /// from a user blocked for rooms is stored hidden and goes no further.
  async fn room_message(&self, room: String, username: String, message: String) {
    if self.rooms.tracker().get(&room).is_none() {
      return debug!(room, "message for a room we are not in dropped");
    }
    let us = self.us_logged_in();
    let is_self = username == us;
    let arrived = Arrived {
      hidden: !is_self && self.users.is_blocked(&username, None, BlockScope::Rooms),
      is_mention: !is_self && mentions::mentions(&message, &us),
      is_self,
      room: room.clone(),
      timestamp: Utc::now(),
      username: username.clone(),
      message,
    };
    let stored = match self
      .db
      .write(move |conn| history::insert(conn, &arrived))
      .await
    {
      Ok(stored) => stored,
      Err(e) => return warn!(room, username, error = %e, "room message not stored"),
    };
    trace!(
      room,
      username,
      local_id = stored.local_id,
      text = stored.message,
      "room message stored"
    );
    if stored.hidden {
      return debug!(room, username, "room message from a blocked user hidden");
    }
    self.users.touch(&username, Touch::RoomMessage).await;
    self
      .events
      .raise(BusEvent::RoomMessageReceived(RoomMessageReceivedEvent {
        version: 0,
        message: stored.clone(),
      }));
    self
      .live
      .publish(LiveFrame::RoomsMessage(RoomMessageReceived {
        room_name: room,
        message: stored,
      }));
    if !is_self {
      self.unread.changed();
    }
  }
}
