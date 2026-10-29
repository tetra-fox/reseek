//! Rooms: joining and leaving, at every login (the configured rooms, then every room someone
//! asked to join and nobody left, before a restart too) and when the configuration adds a room,
//! what is said and who is there, the tickers, the private rooms we belong to and the server's room
//! list. Each room's stored history is read in `stored`.

mod events;
pub mod history;
mod private;
mod stored;
pub mod tracker;

pub use events::room_of;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use api_types::live::LiveFrame;
use api_types::messaging::NotifyLevel;
use api_types::messaging::rooms::{
  JoinRoom, JoinedRoom, Room, RoomInfo, RoomMessagesQuery, RoomTicker, RoomUser,
};
use chrono::Utc;
use soulseek_client::{ClientError, RoomErrorKind};
use soulseek_proto::RoomType;
use soulseek_proto::server::RoomList;
use tracing::{Instrument, debug, info, info_span, warn};

use super::Messaging;
use super::conversations::unsent;
use crate::api::error::ApiError;
use crate::db::DbError;
use crate::services::Joined;
use crate::sync::lock;
use tracker::{Tracker, country_of};

/// How long the server's room list is served again before it is asked for anew.
const LIST_FOR: Duration = Duration::from_secs(60);

/// How many of a room's latest messages a room is shown with.
const LATEST: u32 = 250;

/// The rooms we are in and the server's room list as last asked for.
#[derive(Default)]
pub struct Rooms {
  tracker: Mutex<Tracker>,
  available: tokio::sync::Mutex<Option<(Instant, Arc<RoomList>)>>,
}

impl Rooms {
  pub(in crate::messaging) fn tracker(&self) -> MutexGuard<'_, Tracker> {
    lock(&self.tracker)
  }
}

fn internal(error: DbError) -> ApiError {
  ApiError::internal(error)
}

fn not_in(room: &str) -> ApiError {
  ApiError::not_found(format!("we are not in the room {room}"))
}

/// The answer to a join the server refused.
fn refused(error: ClientError) -> ApiError {
  match error {
    ClientError::Room {
      kind: RoomErrorKind::JoinForbidden,
      ..
    } => ApiError::forbidden("A private room with this name already exists"),
    ClientError::InvalidArgument(message) => ApiError::bad_request(message),
    other => unsent(other),
  }
}

/// Whether a join failed for a reason that holds at every later login too: a private room we
/// are not in holds the name, or the name is not one the server takes.
fn gone(error: &ClientError) -> bool {
  matches!(
    error,
    ClientError::Room {
      kind: RoomErrorKind::JoinForbidden,
      ..
    } | ClientError::InvalidArgument(_)
  )
}

impl Messaging {
  /// Whether the room `name` is tracked.
  fn tracks(&self, name: &str) -> Result<(), ApiError> {
    match self.rooms.tracker().get(name) {
      Some(_) => Ok(()),
      None => Err(not_in(name)),
    }
  }

  /// `State.rooms` as the tracker has them.
  fn rooms_changed(&self) {
    let joined = self.rooms.tracker().joined_names();
    self.state.update(|state| state.rooms = joined);
  }

  /// The connection was lost: no room has us until we join again.
  pub(super) fn rooms_disconnected(&self) {
    self.rooms.tracker().disconnected();
    self.rooms_changed();
  }

  /// Joins `name`, as a private room when it is created and `private`. A server that has us in
  /// the room already, which it says by not answering, is made to forget it by leaving, and the
  /// room is joined again once. A join someone `asked` for through the UI or the API is
  /// remembered, so the room is joined again after a start until it is left.
  async fn join_now(&self, name: &str, private: bool, asked: bool) -> Result<Room, ClientError> {
    self
      .join_room(name, private, asked)
      .instrument(info_span!("room", room = name))
      .await
  }

  async fn join_room(&self, name: &str, private: bool, asked: bool) -> Result<Room, ClientError> {
    let room_type = if private {
      RoomType::Private
    } else {
      RoomType::Public
    };
    let client = self.client();
    // the session's name marks us among the members; taken before the join is sent
    let us = client.own_username(&format!("join chat room {name}"))?;
    let data = match client.join_room(name, room_type).await {
      Err(ClientError::NoResponse(_)) => {
        debug!(
          room = name,
          "the server has us in the room already; left and joined again"
        );
        if let Err(e) = client.leave_room(name).await {
          debug!(room = name, error = %e, "room not left before joining it again");
        }
        client.join_room(name, room_type).await?
      }
      joined => joined?,
    };
    let room = {
      let mut tracker = self.rooms.tracker();
      tracker.joined(&data, &us);
      tracker.room(name).expect("the room was just tracked")
    };
    self.rooms_changed();
    let countries = data
      .users
      .iter()
      .filter_map(|user| Some((user.username.clone(), country_of(user)?.to_string())))
      .collect();
    self.users.countries_seen(countries).await;
    let room_name = name.to_string();
    let now = Utc::now();
    if let Err(e) = self
      .db
      .write(move |conn| history::joined(conn, &room_name, now, asked))
      .await
    {
      warn!(room = name, error = %e, "room join not noted");
    }
    self
      .live
      .publish(LiveFrame::RoomsUpdate(Box::new(room.clone())));
    self.push_rooms().await;
    Ok(room)
  }

  /// Pushes the rooms we are in.
  async fn push_rooms(&self) {
    match self.joined_rooms().await {
      Ok(rooms) => self.live.publish(LiveFrame::RoomsList(rooms)),
      Err(e) => warn!(error = ?e, "rooms not read to push them"),
    }
    self.unread.changed();
  }

  /// After a login: the configured rooms are joined, then the rooms we were in before the
  /// connection was lost, then the rooms someone asked to join before the last stop and nobody
  /// left, and the gap since the last session is pushed. None of these joins is remembered
  /// anew. A room of ours, not of the configuration, that refuses us or whose name the server
  /// takes no more is forgotten.
  pub(super) fn rejoin(&self) {
    let configured = self.config.current().values.rooms.clone();
    let tracked: Vec<String> = self
      .rooms
      .tracker()
      .rooms()
      .iter()
      .map(|room| room.name.clone())
      .collect();
    let this = self.this();
    self.tasks.spawn(async move {
      let remembered = match this.db.read(history::remembered).await {
        Ok(remembered) => remembered,
        Err(e) => {
          warn!(error = %e, "the rooms joined before the last stop not read; not joined again");
          Vec::new()
        }
      };
      let mut ours = Vec::new();
      for room in tracked.into_iter().chain(remembered) {
        if !configured.contains(&room) && !ours.contains(&room) {
          ours.push(room);
        }
      }
      for room in configured {
        match this.join_now(&room, false, false).await {
          Ok(_) => info!(room, "room joined"),
          Err(e) => warn!(room, error = %e, "room not joined"),
        }
      }
      for room in ours {
        match this.join_now(&room, false, false).await {
          Ok(_) => info!(room, "room joined"),
          Err(e) if gone(&e) => {
            warn!(room, error = %e, "room we were in can no longer be joined; forgotten");
            this.forgotten(&room).await;
          }
          Err(e) => warn!(room, error = %e, "room not joined"),
        }
      }
      this.push_gap().await;
    });
  }

  /// The configured rooms changed from `previous` to `current`: the rooms added are joined
  /// now while logged in; the rooms taken off are not left.
  pub fn configured_rooms_changed(&self, previous: &[String], current: &[String]) {
    let added: Vec<String> = current
      .iter()
      .filter(|room| !previous.contains(room))
      .cloned()
      .collect();
    if added.is_empty() {
      return;
    }
    info!(
      rooms = added.join(", "),
      "Room configuration changed.  Joining any newly added rooms."
    );
    if !self.logged_in_now() {
      return debug!("not logged in; the rooms added are joined at the next login");
    }
    let this = self.this();
    self.tasks.spawn(async move {
      for room in added {
        match this.join_now(&room, false, false).await {
          Ok(_) => info!(room, "room joined"),
          Err(e) => warn!(room, error = %e, "room not joined"),
        }
      }
    });
  }

  pub(super) async fn join(&self, request: JoinRoom) -> Result<Joined, ApiError> {
    if self
      .rooms
      .tracker()
      .get(&request.name)
      .is_some_and(|room| room.joined)
    {
      return Ok(Joined::Already);
    }
    let room = self
      .join_now(&request.name, request.private, true)
      .await
      .map_err(refused)?;
    Ok(Joined::New(room))
  }

  pub(super) async fn leave(&self, room: &str) -> Result<(), ApiError> {
    self
      .leave_room(room)
      .instrument(info_span!("room", room))
      .await
  }

  async fn leave_room(&self, room: &str) -> Result<(), ApiError> {
    let joined = match self.rooms.tracker().get(room) {
      Some(tracked) => tracked.joined,
      None => return Err(not_in(room)),
    };
    // a room the lost connection took us out of is only forgotten
    if joined {
      self.client().leave_room(room).await.map_err(unsent)?;
    }
    self.forgotten(room).await;
    Ok(())
  }

  /// Takes `room` off the rooms we are in and the ones joined again after a start.
  async fn forgotten(&self, room: &str) {
    self.rooms.tracker().left(room);
    self.rooms_changed();
    let room_name = room.to_string();
    let now = Utc::now();
    if let Err(e) = self
      .db
      .write(move |conn| history::left(conn, &room_name, now))
      .await
    {
      warn!(room, error = %e, "room leave not noted");
    }
    self.push_rooms().await;
  }

  pub(super) async fn send_room(&self, room: &str, message: String) -> Result<(), ApiError> {
    self.tracks(room)?;
    self
      .client()
      .send_room_message(room, &message)
      .await
      .map_err(unsent)
  }

  pub(super) async fn set_ticker(&self, room: &str, ticker: String) -> Result<(), ApiError> {
    self.tracks(room)?;
    self
      .client()
      .set_room_ticker(room, &ticker)
      .await
      .map_err(unsent)
  }

  pub(super) fn tickers(&self, room: &str) -> Result<Vec<RoomTicker>, ApiError> {
    let tracker = self.rooms.tracker();
    tracker
      .room(room)
      .map(|room| room.tickers)
      .ok_or_else(|| not_in(room))
  }

  pub(super) async fn room(&self, name: &str) -> Result<Room, ApiError> {
    let mut room = self
      .rooms
      .tracker()
      .room(name)
      .ok_or_else(|| not_in(name))?;
    let query = RoomMessagesQuery {
      before: None,
      limit: LATEST,
      include_hidden: false,
    };
    let room_name = name.to_string();
    let (messages, _) = self
      .db
      .read(move |conn| history::page(conn, &room_name, &query))
      .await
      .map_err(internal)?;
    room.messages = Some(messages);
    Ok(room)
  }

  pub(super) fn room_users(&self, room: &str) -> Result<Vec<RoomUser>, ApiError> {
    let tracker = self.rooms.tracker();
    tracker
      .room(room)
      .map(|room| room.users)
      .ok_or_else(|| not_in(room))
  }

  pub(super) async fn joined_rooms(&self) -> Result<Vec<JoinedRoom>, ApiError> {
    let rooms: Vec<(String, u32)> = self
      .rooms
      .tracker()
      .rooms()
      .iter()
      .map(|room| {
        (
          room.name.clone(),
          u32::try_from(room.users.len()).unwrap_or(u32::MAX),
        )
      })
      .collect();
    self
      .db
      .read(move |conn| {
        rooms
          .into_iter()
          .map(|(name, user_count)| {
            let unread = history::unread(conn, &name)?;
            Ok(JoinedRoom {
              notify: history::notify(conn, &name)?,
              last_message_at: history::newest(conn, &name)?,
              last_read_id: history::read_marker(conn, &name)?,
              unread: unread.unread,
              mentions: unread.mentions,
              user_count,
              name,
            })
          })
          .collect()
      })
      .await
      .map_err(internal)
  }

  pub(super) async fn available_rooms(&self) -> Result<Vec<RoomInfo>, ApiError> {
    let list = {
      let mut cached = self.rooms.available.lock().await;
      match cached.as_ref().filter(|(at, _)| at.elapsed() < LIST_FOR) {
        Some((_, list)) => list.clone(),
        None => {
          let list = self.client().get_room_list().await.map_err(unsent)?;
          debug!(
            public = list.public.len(),
            owned = list.owned.len(),
            member = list.private.len(),
            "room list asked of the server"
          );
          *cached = Some((Instant::now(), list.clone()));
          list
        }
      }
    };
    let tracker = self.rooms.tracker();
    let info = |room: &soulseek_proto::server::RoomInfo, private: bool, owned: bool| RoomInfo {
      name: room.name.clone(),
      user_count: room.user_count,
      is_private: private,
      is_owned: owned,
      is_moderated: list.moderated_room_names.contains(&room.name),
      joined: tracker.get(&room.name).is_some_and(|r| r.joined),
    };
    let mut rooms: Vec<RoomInfo> = list.public.iter().map(|r| info(r, false, false)).collect();
    rooms.extend(list.private.iter().map(|r| info(r, true, false)));
    rooms.extend(list.owned.iter().map(|r| info(r, true, true)));
    Ok(rooms)
  }

  pub(super) async fn mark_read(&self, room: &str, local_id: i64) -> Result<(), ApiError> {
    self.tracks(room)?;
    let name = room.to_string();
    self
      .db
      .write(move |conn| history::mark_read(conn, &name, local_id))
      .await
      .map_err(internal)?;
    self.unread.changed();
    Ok(())
  }

  pub(super) async fn set_room_notify(
    &self,
    room: &str,
    notify: NotifyLevel,
  ) -> Result<(), ApiError> {
    self.tracks(room)?;
    let name = room.to_string();
    self
      .db
      .write(move |conn| history::set_notify(conn, &name, notify))
      .await
      .map_err(internal)?;
    self.push_rooms().await;
    Ok(())
  }
}
