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
  /// no try to join the room again after a login got us in; joining it is left to the user
  pub refused: bool,
  /// who is there, by username
  pub users: BTreeMap<String, RoomUser>,
  /// the users' tickers, by username
  pub tickers: BTreeMap<String, String>,
  /// the users whose tickers were last handed out hidden
  pub hidden: BTreeSet<String>,
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

/// A change to a room's members the server told of.
#[derive(Clone, Debug)]
enum MemberChange {
  Joined(RoomUser),
  Left(String),
  Status(String, UserPresence),
}

impl MemberChange {
  /// Makes the change to `users`; a presence is taken only for a user there.
  fn apply(self, users: &mut BTreeMap<String, RoomUser>) {
    match self {
      Self::Joined(user) => {
        users.insert(user.username.clone(), user);
      }
      Self::Left(username) => {
        users.remove(&username);
      }
      Self::Status(username, status) => {
        if let Some(user) = users.get_mut(&username) {
          user.status = status;
        }
      }
    }
  }
}

/// What the server said in a room while our join of it was under way, between its answer and
/// the answer being taken in: it comes behind the answer but may be handled first.
#[derive(Debug, Default)]
struct Joining {
  /// the member changes, in the order they came
  members: Vec<MemberChange>,
  /// the tickers, for a room not tracked yet
  tickers: BTreeMap<String, String>,
}

/// Every room tracked.
#[derive(Debug, Default)]
pub struct Tracker {
  rooms: Vec<Tracked>,
  private: BTreeMap<String, PrivateRoom>,
  /// how many times each room was left in this run, so a join under way meanwhile is undone
  leaves: BTreeMap<String, u64>,
  /// the rooms a join is under way for, with what the server said in them meanwhile
  joining: BTreeMap<String, Joining>,
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

  /// A join of `name` is under way: what the server says in the room from now on is held for
  /// the room until the join's answer is taken in. Anything held from before is dropped.
  pub fn joining(&mut self, name: &str) {
    self.joining.insert(name.to_string(), Joining::default());
  }

  /// The join of `name` under way failed or was undone: nothing more is held for it.
  pub fn not_joined(&mut self, name: &str) {
    self.joining.remove(name);
  }

  /// Whether a message said in `name` is ours to keep: we are in the room, were before the
  /// connection was lost, or are joining it.
  pub fn hears(&self, name: &str) -> bool {
    self.get(name).is_some() || self.joining.contains_key(name)
  }

  /// We joined `data`'s room: its members replace any we knew, then what the server said in the
  /// room while the join was under way is taken in order. Its tickers stay until the server
  /// sends them again, and for a private room its owner and operators are taken.
  pub fn joined(&mut self, data: &RoomData, us: &str) {
    let mut users: BTreeMap<String, RoomUser> = data
      .users
      .iter()
      .map(|user| (user.username.clone(), room_user(user, us)))
      .collect();
    let early = self.joining.remove(&data.name).unwrap_or_default();
    for change in early.members {
      change.apply(&mut users);
    }
    match self.get_mut(&data.name) {
      Some(room) => {
        room.joined = true;
        room.refused = false;
        room.users = users;
      }
      None => self.rooms.push(Tracked {
        name: data.name.clone(),
        joined: true,
        refused: false,
        users,
        tickers: early.tickers,
        hidden: BTreeSet::new(),
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

  /// Keeps `name` among the rooms, not joined, after a join of it after a login failed, unless
  /// it was left since `leaves` was read; says whether it was added.
  pub fn keep(&mut self, name: &str, leaves: u64) -> bool {
    if self.get(name).is_some() || self.leaves(name) != leaves {
      return false;
    }
    self.rooms.push(Tracked {
      name: name.to_string(),
      ..Tracked::default()
    });
    true
  }

  /// Marks `name` refused: no try to join it again after a login got us in. False when it is
  /// not among the rooms or is joined.
  pub fn mark_refused(&mut self, name: &str) -> bool {
    match self.get_mut(name).filter(|room| !room.joined) {
      Some(room) => {
        room.refused = true;
        true
      }
      None => false,
    }
  }

  /// We left `name`; false when it was not tracked.
  pub fn left(&mut self, name: &str) -> bool {
    *self.leaves.entry(name.to_string()).or_default() += 1;
    self.joining.remove(name);
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

  /// Takes `change` to `room`'s members: at once in a room the server has us in, held for the
  /// join's answer in a room being joined; false when we are in neither.
  fn member_change(&mut self, room: &str, change: MemberChange) -> bool {
    if let Some(tracked) = self.get_mut(room).filter(|tracked| tracked.joined) {
      change.apply(&mut tracked.users);
      return true;
    }
    match self.joining.get_mut(room) {
      Some(joining) => {
        joining.members.push(change);
        true
      }
      None => false,
    }
  }

  /// `user` joined `room`; false when we are not in the room nor joining it.
  pub fn user_joined(&mut self, room: &str, user: RoomUser) -> bool {
    self.member_change(room, MemberChange::Joined(user))
  }

  /// `username` left `room`; false when we are not in the room nor joining it.
  pub fn user_left(&mut self, room: &str, username: &str) -> bool {
    self.member_change(room, MemberChange::Left(username.to_string()))
  }

  /// `username` is `status` now; the rooms we are in where their presence changed, in the order
  /// joined. The rooms being joined take it with their join's answer.
  pub fn status(&mut self, username: &str, status: UserPresence) -> Vec<String> {
    for joining in self.joining.values_mut() {
      joining
        .members
        .push(MemberChange::Status(username.to_string(), status));
    }
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

  /// Changes `room`'s tickers with `change`, those of a room being joined for the first time
  /// held for its join's answer; false when the room is neither tracked nor being joined.
  pub fn tickers(
    &mut self,
    room: &str,
    change: impl FnOnce(&mut BTreeMap<String, String>),
  ) -> bool {
    if let Some(tracked) = self.get_mut(room) {
      change(&mut tracked.tickers);
      return true;
    }
    match self.joining.get_mut(room) {
      Some(joining) => {
        change(&mut joining.tickers);
        true
      }
      None => false,
    }
  }

  /// Takes `hidden` as the users whose tickers in `room` were last handed out hidden.
  pub fn handed_out_hidden(&mut self, room: &str, hidden: BTreeSet<String>) {
    if let Some(tracked) = self.get_mut(room) {
      tracked.hidden = hidden;
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
  fn what_a_room_says_while_its_join_is_under_way_is_taken_with_the_answer() {
    let names = |tracker: &Tracker| -> Vec<String> {
      let room = tracker.room("indie").unwrap();
      room.users.into_iter().map(|u| u.username).collect()
    };
    let mut tracker = Tracker::default();
    tracker.joining("indie");
    assert!(tracker.hears("indie"));
    assert!(tracker.tickers("indie", |t| {
      t.insert("alice".into(), "away".into());
    }));
    assert!(tracker.user_joined("indie", room_user(&user("bob"), "me")));
    assert!(tracker.user_left("indie", "alice"));
    let mut data = RoomData {
      name: "indie".into(),
      users: vec![user("me"), user("alice")],
      private: None,
    };
    tracker.joined(&data, "me");
    assert_eq!(names(&tracker), ["bob", "me"]);
    assert_eq!(tracker.room("indie").unwrap().tickers.len(), 1);

    // the answer to the join after a lost connection is overtaken the same way
    tracker.disconnected();
    assert!(
      !tracker.user_joined("indie", room_user(&user("carol"), "me")),
      "not in the room until it is joined again"
    );
    tracker.joining("indie");
    assert!(tracker.user_left("indie", "bob"));
    data.users = vec![user("me"), user("bob")];
    tracker.joined(&data, "me");
    assert_eq!(names(&tracker), ["me"]);
    tracker.joining("jazz");
    tracker.not_joined("jazz");
    assert!(!tracker.hears("jazz"));
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
  fn a_room_refused_after_a_login_is_kept_unless_left_and_a_join_ends_the_refusal() {
    let mut tracker = Tracker::default();
    let leaves = tracker.leaves("indie");
    assert!(tracker.keep("indie", leaves));
    assert!(!tracker.keep("indie", leaves), "kept once");
    assert!(tracker.mark_refused("indie"));
    assert!(tracker.get("indie").unwrap().refused);
    let data = RoomData {
      name: "indie".into(),
      users: vec![user("me")],
      private: None,
    };
    tracker.joined(&data, "me");
    assert!(!tracker.get("indie").unwrap().refused);
    assert!(
      !tracker.mark_refused("indie"),
      "a joined room is not refused"
    );

    // a room left while its join after the login was under way is not kept
    let leaves = tracker.leaves("jazz");
    tracker.left("jazz");
    assert!(!tracker.keep("jazz", leaves));
    assert!(tracker.get("jazz").is_none());
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
