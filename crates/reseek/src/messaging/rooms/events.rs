//! What the server tells us about rooms: who joins and leaves, what is said, the tickers, and
//! the private rooms we belong to. A message is stored before anyone is told of it; one for a
//! room we are not in is dropped, and one from a user blocked for rooms is stored hidden and
//! told to no one. A change of a block pushes again the rooms whose tickers it hides or shows.

use std::collections::BTreeMap;

use api_types::events::{Event as BusEvent, EventType, RoomMessageReceivedEvent};
use api_types::live::LiveFrame;
use api_types::messaging::rooms::{
  RoomMessageReceived, RoomUserJoined, RoomUserLeft, RoomUserStatus,
};
use api_types::users::UserPresence;
use api_types::users::blocking::BlockScope;
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
        self.forget_room_list().await;
        self.push_rooms().await;
      }
      Event::PrivateRoomMembershipRemoved(room) => {
        info!(room, "Removed from private room");
        self.dropped(&room).await;
      }
      Event::PrivateRoomModerationAdded(room) => {
        info!(room, "Promoted to moderator in private room");
        self.forget_room_list().await;
        if let Some(us) = self.logged_in_as() {
          self.change_private(&room, |private| {
            private.operators.insert(us);
          });
        }
      }
      Event::PrivateRoomModerationRemoved(room) => {
        info!(room, "Demoted from moderator in private room");
        self.forget_room_list().await;
        if let Some(us) = self.logged_in_as() {
          self.change_private(&room, |private| {
            private.operators.remove(&us);
          });
        }
      }
      other => trace!(?other, "client event no room engine follows"),
    }
  }

  /// Pushes the rooms whose tickers a block now hides or shows whenever a block is made,
  /// changed or ends, so their walls follow at once. Every room is checked, not only those the
  /// user is in, since a block naming addresses can hide any member.
  pub(in crate::messaging) fn follow_blocks(&self) {
    for kind in [EventType::UserBlocked, EventType::UserUnblocked] {
      let messaging = self.this.clone();
      self.events.subscribe("rooms", Some(kind), move |_| {
        // the engine is dropped only as the daemon ends
        if let Some(messaging) = messaging.upgrade() {
          messaging.push_rooms_again();
        }
        async { Ok(()) }
      });
    }
  }

  /// Pushes the rooms we are in whose tickers would be marked otherwise than when they were
  /// last handed out.
  fn push_rooms_again(&self) {
    let moved: Vec<String> = {
      let tracker = self.rooms.tracker();
      tracker
        .rooms()
        .iter()
        .filter(|room| self.hidden_of(room.tickers.keys()) != room.hidden)
        .map(|room| room.name.clone())
        .collect()
    };
    if moved.is_empty() {
      return;
    }
    debug!(rooms = ?moved, "rooms whose tickers a block hides or shows pushed again");
    for name in &moved {
      self.push_room(name);
    }
  }

  /// Pushes the room `name` as it is now, when it is tracked.
  fn push_room(&self, name: &str) {
    let room = self.rooms.tracker().room(name);
    if let Some(room) = room {
      let room = self.marked(room);
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
    // room events follow their session's login, unless the login was among events missed
    let Some(us) = self.logged_in_as() else {
      return debug!(room, "a join before any login was noted dropped");
    };
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

  /// A member of the rooms we are in is `status` now: pushed for each room where that changes
  /// their presence, so a pane can say they went away or came back.
  pub fn member_status(&self, username: &str, status: UserPresence) {
    let rooms = self.rooms.tracker().status(username, status);
    for room_name in rooms {
      trace!(
        room = room_name,
        username,
        ?status,
        "room member's presence changed"
      );
      self
        .live
        .publish(LiveFrame::RoomsUserStatus(RoomUserStatus {
          room_name,
          username: username.to_string(),
          status,
        }));
    }
  }

  /// `username` left `room`. Our own leave is the one we asked for, which has taken the room
  /// off already.
  fn user_left(&self, room: String, username: String) {
    if self.logged_in_as().as_deref() == Some(username.as_str()) {
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
    // room events follow their session's login, unless the login was among events missed
    let Some(us) = self.logged_in_as() else {
      return debug!(room, "a room message before any login was noted dropped");
    };
    let is_self = username == us;
    let options = self.config.current();
    let watched = &options.values.chat.highlight_words;
    let arrived = Arrived {
      hidden: !is_self && self.users.is_blocked(&username, None, BlockScope::Rooms),
      is_mention: !is_self && mentions::mentions(&message, &us, watched),
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

#[cfg(test)]
mod tests {
  use soulseek_client::Event;
  use soulseek_proto::server::RoomData;

  use super::history;
  use crate::sync::lock;
  use crate::testing::app;

  #[tokio::test]
  async fn a_login_handled_after_its_session_ended_names_us() {
    // no username configured and the client logged out: only the login's own name says who we
    // are, not the earlier account's
    let app = app("", &[]).await;
    let messaging = &app.state.messaging;
    *lock(&messaging.session_user) = Some("old".into());
    messaging
      .handle(Event::LoggedIn {
        session: 1,
        username: "me".into(),
      })
      .await;
    let data = RoomData {
      name: "indie".into(),
      users: Vec::new(),
      private: None,
    };
    messaging.rooms.tracker().joined(&data, "me");
    for (username, message) in [("me", "hello"), ("old", "hi me")] {
      let said = Event::RoomMessageReceived {
        room_name: "indie".into(),
        username: username.into(),
        message: message.into(),
      };
      messaging.room_event(said).await;
    }
    let stored = messaging
      .database()
      .read(|conn| history::all(conn, "indie"))
      .await
      .unwrap();
    assert_eq!(
      stored
        .iter()
        .map(|m| (m.is_self, m.is_mention))
        .collect::<Vec<_>>(),
      [(true, false), (false, true)]
    );
  }

  #[tokio::test]
  async fn a_room_message_with_a_watched_word_is_stored_as_a_mention() {
    let app = app("[chat]\nhighlight_words = [\"Floyd\"]\n", &[]).await;
    let messaging = &app.state.messaging;
    messaging
      .handle(Event::LoggedIn {
        session: 1,
        username: "me".into(),
      })
      .await;
    let data = RoomData {
      name: "indie".into(),
      users: Vec::new(),
      private: None,
    };
    messaging.rooms.tracker().joined(&data, "me");
    for message in ["pink floyd tonight", "floydian slip"] {
      let said = Event::RoomMessageReceived {
        room_name: "indie".into(),
        username: "bob".into(),
        message: message.into(),
      };
      messaging.room_event(said).await;
    }
    let stored = messaging
      .database()
      .read(|conn| history::all(conn, "indie"))
      .await
      .unwrap();
    let mentions: Vec<_> = stored.iter().map(|m| m.is_mention).collect();
    assert_eq!(mentions, [true, false]);
  }

  #[tokio::test]
  async fn the_ticker_of_a_user_hidden_in_rooms_is_marked_hidden() {
    let app = app(
      "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    )
    .await;
    let block = serde_json::json!({ "scopes": ["rooms"] });
    let answer = app
      .send(crate::testing::request(
        "PUT",
        "/api/v0/users/mallory/block",
        Some(block),
      ))
      .await;
    assert_eq!(answer.status, 200);
    let messaging = &app.state.messaging;
    let data = RoomData {
      name: "indie".into(),
      users: Vec::new(),
      private: None,
    };
    messaging.rooms.tracker().joined(&data, "me");
    for username in ["mallory", "bob"] {
      let set = Event::RoomTickerAdded {
        room_name: "indie".into(),
        ticker: soulseek_proto::server::RoomTicker {
          username: username.into(),
          message: "listening".into(),
        },
      };
      messaging.room_event(set).await;
    }
    let tickers = messaging.tickers("indie").unwrap();
    let hidden: Vec<_> = tickers
      .iter()
      .map(|t| (t.username.as_str(), t.hidden))
      .collect();
    assert_eq!(hidden, [("bob", false), ("mallory", true)]);
  }

  #[tokio::test]
  async fn a_block_and_its_end_reach_an_open_wall_at_once() {
    let app = app(
      "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    )
    .await;
    let messaging = &app.state.messaging;
    let data = RoomData {
      name: "indie".into(),
      users: Vec::new(),
      private: None,
    };
    messaging.rooms.tracker().joined(&data, "me");
    let set = Event::RoomTickerAdded {
      room_name: "indie".into(),
      ticker: soulseek_proto::server::RoomTicker {
        username: "mallory".into(),
        message: "listening".into(),
      },
    };
    messaging.room_event(set).await;
    let mut stream = app
      .stream_from(
        "127.0.0.1:40000",
        crate::testing::request("GET", "/api/v0/live?topics=rooms", None),
      )
      .await;
    stream.until("hello").await;
    // each change of the block pushes the room, its ticker marked as the block now says
    let mut wall_says = async |hidden: bool| loop {
      let frame = stream.until("rooms.update").await.pop().unwrap();
      if frame.data["tickers"][0]["hidden"] == hidden {
        return;
      }
    };
    let block = serde_json::json!({ "scopes": ["rooms"] });
    let path = "/api/v0/users/mallory/block";
    let answer = app
      .send(crate::testing::request("PUT", path, Some(block)))
      .await;
    assert_eq!(answer.status, 200);
    wall_says(true).await;
    let answer = app
      .send(crate::testing::request("DELETE", path, None))
      .await;
    assert_eq!(answer.status, 204);
    wall_says(false).await;
  }

  #[tokio::test]
  async fn a_block_that_hides_no_ticker_pushes_no_room() {
    let app = app(
      "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    )
    .await;
    let messaging = &app.state.messaging;
    let data = RoomData {
      name: "indie".into(),
      users: Vec::new(),
      private: None,
    };
    messaging.rooms.tracker().joined(&data, "me");
    let set = Event::RoomTickerAdded {
      room_name: "indie".into(),
      ticker: soulseek_proto::server::RoomTicker {
        username: "mallory".into(),
        message: "listening".into(),
      },
    };
    messaging.room_event(set).await;
    let mut stream = app
      .stream_from(
        "127.0.0.1:40000",
        crate::testing::request("GET", "/api/v0/live?topics=rooms", None),
      )
      .await;
    stream.until("hello").await;
    // eve has no ticker and mallory's block is only for uploads, so neither changes the wall;
    // the blocks are handled in order, so the first room pushed is the one mallory's hides
    let blocks = [
      ("eve", serde_json::json!({ "scopes": ["rooms"] })),
      ("mallory", serde_json::json!({ "scopes": ["uploads"] })),
      ("mallory", serde_json::json!({ "scopes": ["rooms"] })),
    ];
    for (username, block) in blocks {
      let path = format!("/api/v0/users/{username}/block");
      let answer = app
        .send(crate::testing::request("PUT", &path, Some(block)))
        .await;
      assert_eq!(answer.status, 200);
    }
    let frame = stream.until("rooms.update").await.pop().unwrap();
    assert_eq!(frame.data["tickers"][0]["hidden"], true);
  }

  #[tokio::test]
  async fn a_private_room_we_were_dropped_from_is_not_joined_again() {
    let app = app("", &[]).await;
    let messaging = &app.state.messaging;
    messaging
      .database()
      .write(|conn| history::joined(conn, "club", chrono::Utc::now(), true))
      .await
      .unwrap();
    messaging
      .handle(Event::PrivateRoomMembershipRemoved("club".into()))
      .await;
    let remembered = messaging
      .database()
      .read(history::remembered)
      .await
      .unwrap();
    assert!(remembered.is_empty(), "{remembered:?}");
  }
}
