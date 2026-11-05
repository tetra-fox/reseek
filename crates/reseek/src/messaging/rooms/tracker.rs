//! The rooms we are in, in the order we joined them, with who is there and their tickers, and
//! the private rooms we belong to with their owners, members and operators. Kept in memory: a
//! room stays across a lost connection so it is joined again, and is gone once left or at a
//! restart.

use std::collections::{BTreeMap, BTreeSet};

use api_types::messaging::rooms::{Room, RoomTicker, RoomUser};
use api_types::state::UserStatistics;
use api_types::users::UserPresence;
use soulseek_proto::server::{RoomData, UserData};

/// A room we are in, or were in before the connection was lost.
#[derive(Clone, Debug, Default)]
pub struct Tracked {
  /// the room's name
  pub name: String,
  /// the server has us in the room in this session
  pub joined: bool,
  /// who is there, by username
  pub users: BTreeMap<String, RoomUser>,
  /// the users' tickers, by username
  pub tickers: BTreeMap<String, String>,
}

/// A private room we belong to, whether we are in it now or not.
#[derive(Clone, Debug, Default)]
pub struct PrivateRoom {
  /// its owner, when a join told us
  pub owner: Option<String>,
  /// its members, the owner not among them
  pub members: BTreeSet<String>,
  /// its operators
  pub operators: BTreeSet<String>,
}

/// Every room tracked.
#[derive(Debug, Default)]
pub struct Tracker {
  rooms: Vec<Tracked>,
  private: BTreeMap<String, PrivateRoom>,
  /// how many times each room was left in this run, so a join under way meanwhile is undone
  leaves: BTreeMap<String, u64>,
}

/// A member's country as the server gave it, none when it sent a blank, as it does for a user
/// who is offline.
pub fn country_of(user: &UserData) -> Option<&str> {
  Some(user.country_code.as_str()).filter(|code| !code.trim().is_empty())
}

/// A member as a room shows them; `us` marks ourselves.
pub fn room_user(user: &UserData, us: &str) -> RoomUser {
  RoomUser {
    username: user.username.clone(),
    status: user.status.into(),
    statistics: UserStatistics {
      average_speed: user.stats.average_speed,
      directory_count: user.stats.directory_count,
      file_count: user.stats.file_count,
      upload_count: user.stats.upload_count.into(),
    },
    slots_full: Some(user.slots_full),
    country_code: country_of(user).map(str::to_string),
    is_self: user.username == us,
  }
}

impl Tracker {
  /// The room `name`.
  pub fn get(&self, name: &str) -> Option<&Tracked> {
    self.rooms.iter().find(|room| room.name == name)
  }

  fn get_mut(&mut self, name: &str) -> Option<&mut Tracked> {
    self.rooms.iter_mut().find(|room| room.name == name)
  }

  /// Every room, in the order joined.
  pub fn rooms(&self) -> &[Tracked] {
    &self.rooms
  }

  /// The names of the rooms the server has us in now, in the order joined.
  pub fn joined_names(&self) -> Vec<String> {
    self
      .rooms
      .iter()
      .filter(|room| room.joined)
      .map(|room| room.name.clone())
      .collect()
  }

  /// We joined `data`'s room: its members replace any we knew, its tickers stay until the
  /// server sends them again, and for a private room its owner and operators are taken.
  pub fn joined(&mut self, data: &RoomData, us: &str) {
    let users = data
      .users
      .iter()
      .map(|user| (user.username.clone(), room_user(user, us)))
      .collect();
    match self.get_mut(&data.name) {
      Some(room) => {
        room.joined = true;
        room.users = users;
      }
      None => self.rooms.push(Tracked {
        name: data.name.clone(),
        joined: true,
        users,
        tickers: BTreeMap::new(),
      }),
    }
    if let Some(private) = &data.private {
      let record = self.private.entry(data.name.clone()).or_default();
      record.owner = Some(private.owner.clone());
      record.operators = private.operators.iter().cloned().collect();
      record.members = data
        .users
        .iter()
        .map(|user| user.username.clone())
        .filter(|name| *name != private.owner)
        .collect();
    }
  }

  /// The server confirmed we are in `name`; false when it is not tracked.
  pub fn confirmed(&mut self, name: &str) -> bool {
    match self.get_mut(name) {
      Some(room) => {
        room.joined = true;
        true
      }
      None => false,
    }
  }

  /// How many times `name` was left so far in this run. A join reads it as it starts and again
  /// as it ends, and a leave in between undoes the join.
  pub fn leaves(&self, name: &str) -> u64 {
    self.leaves.get(name).copied().unwrap_or(0)
  }

  /// We left `name`; false when it was not tracked.
  pub fn left(&mut self, name: &str) -> bool {
    *self.leaves.entry(name.to_string()).or_default() += 1;
    let before = self.rooms.len();
    self.rooms.retain(|room| room.name != name);
    self.rooms.len() != before
  }

  /// The connection was lost: no room has us now, nor anyone in it, until we join again.
  pub fn disconnected(&mut self) {
    for room in &mut self.rooms {
      room.joined = false;
      room.users.clear();
    }
  }

  /// `user` joined `room`; false when the room is not tracked.
  pub fn user_joined(&mut self, room: &str, user: RoomUser) -> bool {
    match self.get_mut(room) {
      Some(tracked) => {
        tracked.users.insert(user.username.clone(), user);
        true
      }
      None => false,
    }
  }

  /// `username` left `room`; false when the room is not tracked.
  pub fn user_left(&mut self, room: &str, username: &str) -> bool {
    match self.get_mut(room) {
      Some(tracked) => {
        tracked.users.remove(username);
        true
      }
      None => false,
    }
  }

  /// `username` is `status` now; the rooms we are in where their presence changed, in the order
  /// joined.
  pub fn status(&mut self, username: &str, status: UserPresence) -> Vec<String> {
    let mut changed = Vec::new();
    for room in self.rooms.iter_mut().filter(|room| room.joined) {
      if let Some(user) = room.users.get_mut(username)
        && user.status != status
      {
        user.status = status;
        changed.push(room.name.clone());
      }
    }
    changed
  }

  /// Changes `room`'s tickers with `change`; false when the room is not tracked.
  pub fn tickers(
    &mut self,
    room: &str,
    change: impl FnOnce(&mut BTreeMap<String, String>),
  ) -> bool {
    match self.get_mut(room) {
      Some(tracked) => {
        change(&mut tracked.tickers);
        true
      }
      None => false,
    }
  }

  /// The private room `name`, when we belong to it.
  pub fn private(&self, name: &str) -> Option<&PrivateRoom> {
    self.private.get(name)
  }

  /// Changes the private room `name` with `change`, making its record when there is none.
  pub fn change_private(&mut self, name: &str, change: impl FnOnce(&mut PrivateRoom)) {
    change(self.private.entry(name.to_string()).or_default());
  }

  /// We no longer belong to the private room `name`, and are no longer in it.
  pub fn drop_private(&mut self, name: &str) -> bool {
    let left = self.left(name);
    self.private.remove(name).is_some() || left
  }

  /// The room `name` as the API shows it, without its messages and with no ticker marked
  /// hidden; the messaging engine marks them.
  pub fn room(&self, name: &str) -> Option<Room> {
    let tracked = self.get(name)?;
    let private = self.private.get(name);
    let operators: Option<Vec<String>> = private.map(|p| p.operators.iter().cloned().collect());
    let members = private.map(|p| {
      let mut members: BTreeSet<String> = p.members.clone();
      members.extend(p.owner.clone());
      members.into_iter().collect()
    });
    Some(Room {
      name: tracked.name.clone(),
      is_private: private.is_some_and(|p| p.owner.is_some()),
      owner: private.and_then(|p| p.owner.clone()),
      operator_count: operators
        .as_ref()
        .map(|list| u32::try_from(list.len()).unwrap_or(u32::MAX)),
      operators,
      members,
      users: tracked.users.values().cloned().collect(),
      tickers: tracked
        .tickers
        .iter()
        .map(|(username, ticker)| RoomTicker {
          username: username.clone(),
          ticker: ticker.clone(),
          hidden: false,
        })
        .collect(),
      messages: None,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use soulseek_proto::UserPresence;
  use soulseek_proto::server::{PrivateRoomData, UserStats};

  fn user(name: &str) -> UserData {
    UserData {
      username: name.into(),
      status: UserPresence::Online,
      stats: UserStats {
        average_speed: 100,
        upload_count: 1,
        file_count: 10,
        directory_count: 2,
      },
      slots_full: false,
      country_code: String::new(),
    }
  }

  #[test]
  fn a_rejoin_replaces_the_members_and_a_join_twice_keeps_one_of_each() {
    let mut tracker = Tracker::default();
    let mut data = RoomData {
      name: "indie".into(),
      users: vec![user("me"), user("alice")],
      private: None,
    };
    tracker.joined(&data, "me");
    assert!(tracker.user_joined("indie", room_user(&user("alice"), "me")));
    assert_eq!(
      tracker.get("indie").unwrap().users.len(),
      2,
      "keyed by name"
    );
    tracker.tickers("indie", |t| {
      t.insert("alice".into(), "away".into());
    });
    tracker.disconnected();
    assert!(!tracker.get("indie").unwrap().joined);
    data.users = vec![user("me"), user("bob")];
    tracker.joined(&data, "me");
    let room = tracker.room("indie").unwrap();
    assert_eq!(
      room
        .users
        .iter()
        .map(|u| (u.username.as_str(), u.is_self))
        .collect::<Vec<_>>(),
      [("bob", false), ("me", true)]
    );
    assert_eq!(room.tickers.len(), 1, "tickers stay until sent again");
    assert!(!tracker.user_joined("rock", room_user(&user("x"), "me")));
  }

  #[test]
  fn a_private_room_lists_its_owner_with_its_members() {
    let mut tracker = Tracker::default();
    let data = RoomData {
      name: "club".into(),
      users: vec![user("owner"), user("me"), user("op")],
      private: Some(PrivateRoomData {
        owner: "owner".into(),
        operators: vec!["op".into()],
      }),
    };
    tracker.joined(&data, "me");
    // the server's list of members leaves the owner out
    tracker.change_private("club", |p| {
      p.members = ["me".to_string(), "op".to_string(), "new".to_string()].into();
    });
    let room = tracker.room("club").unwrap();
    assert!(room.is_private);
    assert_eq!(room.owner.as_deref(), Some("owner"));
    assert_eq!(room.members.unwrap(), ["me", "new", "op", "owner"]);
    assert_eq!(room.operators.unwrap(), ["op"]);
    assert_eq!(room.operator_count, Some(1));
    assert!(tracker.drop_private("club"));
    assert!(tracker.room("club").is_none());
  }

  #[test]
  fn a_presence_change_names_each_joined_room_it_changes() {
    let mut tracker = Tracker::default();
    for name in ["indie", "jazz", "folk"] {
      let data = RoomData {
        name: name.into(),
        users: vec![user("me"), user("alice")],
        private: None,
      };
      tracker.joined(&data, "me");
    }
    tracker.user_left("folk", "alice");
    let away = api_types::users::UserPresence::Away;
    assert_eq!(tracker.status("alice", away), ["indie", "jazz"]);
    assert!(
      tracker.status("alice", away).is_empty(),
      "no change, no news"
    );
    assert!(tracker.status("nobody", away).is_empty());
  }
}
