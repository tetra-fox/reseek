//! Where an upload stands in the queue, and where a new request would land. Places count only
//! the uploads of the same group, those running included, 0-based: higher groups cannot be
//! foreseen cheaply, so a place can be lower than the wait turns out.

use uuid::Uuid;

use super::{State, UploadQueue};

/// Why a group a user is queued in is in the table.
const KNOWN: &str = "users are queued in a group of the table, else in the default group";
use crate::config::enums::QueueStrategy;
use crate::users::DEFAULT_GROUP;

/// Where a new upload request from a user would land in the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forecast {
  /// the group the request would be queued in
  pub group: String,
  /// the uploads that group runs at once
  pub total_slots: u32,
  /// how many of those are free; slots other groups hold are not counted, so a slot can be
  /// free here while every global slot is taken
  pub free_slots: u32,
  /// the place the request would take, 0 when a slot is free
  pub position: u32,
}

/// What a queued upload waits for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
  /// its place among its group's uploads, `None` once it started
  pub position: Option<u32>,
  /// the group it is queued in, or started from
  pub group: String,
  /// the group's slots in use
  pub group_slots_used: u32,
  /// the group's slots
  pub group_slots: u32,
  /// the slots in use over every group
  pub global_slots_used: u32,
  /// the slots over every group
  pub global_slots: u32,
  /// the user's uploads holding a slot
  pub user_active: u32,
}

impl UploadQueue {
  /// The users queued in `group` now, with how many uploads each has in the queue, in the
  /// order each first queued.
  fn group_users(&self, state: &mut State, group: &str) -> Vec<(String, usize)> {
    let users: Vec<(String, usize)> = state
      .users
      .iter()
      .map(|u| (u.username.clone(), u.entries.len()))
      .collect();
    users
      .into_iter()
      .filter(|(username, _)| self.queued_group(state, username) == group)
      .collect()
  }

  /// The place of the upload at `i` of user `u` in `group`.
  fn estimate_at(&self, state: &mut State, group: &str, u: usize, i: usize) -> u32 {
    let strategy = state.group(group).expect(KNOWN).strategy;
    let username = state.users[u].username.clone();
    let others: Vec<(String, usize)> = self
      .group_users(state, group)
      .into_iter()
      .filter(|(name, _)| *name != username)
      .collect();
    let place = match strategy {
      // every user downloads a file per round: the files of the rounds before this one, and,
      // assuming this file is last in its round, every other user with a file in it
      QueueStrategy::RoundRobin => {
        let local = i;
        local
          + others.iter().map(|(_, n)| local.min(*n)).sum::<usize>()
          + others.iter().filter(|(_, n)| *n > local).count()
      }
      QueueStrategy::FirstInFirstOut => {
        let queued = state.users[u].entries[i].queued;
        let mut ahead = state.users[u].entries[..i].len();
        for (name, _) in &others {
          let user = state.users.iter().find(|x| &x.username == name);
          ahead += user.map_or(0, |user| {
            user.entries.iter().filter(|e| e.queued < queued).count()
          });
        }
        ahead
      }
    };
    u32::try_from(place).unwrap_or(u32::MAX)
  }

  /// The place of the latest upload of `filename` to `username`, `None` when the queue holds
  /// none.
  pub fn place(&self, username: &str, filename: &str) -> Option<u32> {
    let mut state = self.lock();
    let u = state.users.iter().position(|x| x.username == username)?;
    let i = state.users[u]
      .entries
      .iter()
      .rposition(|e| e.filename == filename)?;
    let group = self.queued_group(&mut state, username);
    Some(self.estimate_at(&mut state, &group, u, i))
  }

  /// Where a new request from `username` would land, by the group known for them now.
  pub fn forecast(&self, username: &str) -> Forecast {
    let mut state = self.lock();
    let group = self.queued_group(&mut state, username);
    self.forecast_in(&mut state, group)
  }

  /// Where a new request from a user of the default group would land, such as a user we know
  /// nothing about.
  pub fn default_forecast(&self) -> Forecast {
    let mut state = self.lock();
    self.forecast_in(&mut state, DEFAULT_GROUP.to_string())
  }

  /// Where a new request queued in `group` would land.
  fn forecast_in(&self, state: &mut State, group: String) -> Forecast {
    let known = state.group(&group).expect(KNOWN);
    let (strategy, total, used) = (known.strategy, known.slots, known.used.len() as u32);
    let free = total.saturating_sub(used);
    let position = if free > 0 {
      0
    } else {
      let users = self.group_users(state, &group);
      let waiting = match strategy {
        QueueStrategy::RoundRobin => users.len(),
        QueueStrategy::FirstInFirstOut => users.iter().map(|(_, n)| n).sum(),
      };
      u32::try_from(waiting).unwrap_or(u32::MAX)
    };
    Forecast {
      group,
      total_slots: total,
      free_slots: free,
      position,
    }
  }

  /// The slots of the group `username` is queued in and of every group, and the user's
  /// uploads holding one, for an upload the queue does not hold.
  pub fn overview(&self, username: &str) -> Standing {
    let mut state = self.lock();
    let group = self.queued_group(&mut state, username);
    let known = state.group(&group).expect(KNOWN);
    let (group_slots_used, group_slots) = (known.used.len() as u32, known.slots);
    let user_active = state
      .users
      .iter()
      .filter(|u| u.username == username)
      .flat_map(|u| u.entries.iter())
      .filter(|e| e.started.is_some())
      .count() as u32;
    Standing {
      position: None,
      group,
      group_slots_used,
      group_slots,
      global_slots_used: state.used(),
      global_slots: state.global_slots,
      user_active,
    }
  }

  /// What the upload `id` waits for, worked out without changing the queue; `None` when the
  /// queue does not hold it.
  pub fn standing(&self, id: Uuid) -> Option<Standing> {
    let mut state = self.lock();
    let (u, i) = state.find(id)?;
    let username = state.users[u].username.clone();
    let entry = &state.users[u].entries[i];
    let group = match &entry.group {
      Some(started_in) => started_in.clone(),
      None => self.queued_group(&mut state, &username),
    };
    let position = state.users[u].entries[i]
      .started
      .is_none()
      .then(|| self.estimate_at(&mut state, &group, u, i));
    let (group_slots_used, group_slots) = state
      .group(&group)
      .map_or((0, 0), |g| (g.used.len() as u32, g.slots));
    let user_active = state.users[u]
      .entries
      .iter()
      .filter(|e| e.started.is_some())
      .count() as u32;
    Some(Standing {
      position,
      group,
      group_slots_used,
      group_slots,
      global_slots_used: state.used(),
      global_slots: state.global_slots,
      user_active,
    })
  }
}

#[cfg(test)]
mod tests {
  use uuid::Uuid;

  use super::super::tests::{queue, ready, started};
  use super::*;
  use crate::users::DEFAULT_GROUP;

  /// Queues `count` uploads for `username`, none ready; returns their ids.
  fn files(queue: &UploadQueue, username: &str, count: usize) -> Vec<Uuid> {
    (0..count)
      .map(|n| {
        let id = Uuid::new_v4();
        queue.enqueue(id, username, &format!("{username}{n}"));
        id
      })
      .collect()
  }

  #[test]
  fn round_robin_places_count_the_rounds_before_and_everyone_in_this_one() {
    let (queue, _) = queue("");
    files(&queue, "a", 5);
    files(&queue, "b", 2);
    files(&queue, "c", 12);
    let d = files(&queue, "d", 7);
    assert_eq!(queue.place("d", "d4"), Some(16));
    assert_eq!(queue.standing(d[4]).unwrap().position, Some(16));
    assert_eq!(queue.place("d", "nothing"), None);
  }

  #[test]
  fn fifo_places_count_every_upload_of_the_group_queued_before() {
    let (queue, groups) = queue(
      "[transfers.groups.default.upload]\nstrategy = \"firstinfirstout\"\n\
       [transfers.groups.user_defined.other]\n",
    );
    groups.set("x", "other");
    let a = files(&queue, "a", 2);
    files(&queue, "x", 3);
    files(&queue, "b", 2);
    // running uploads count as ahead
    let mut start = queue.await_start(a[0]).unwrap();
    assert!(started(&mut start));
    assert_eq!(queue.place("b", "b1"), Some(3), "a's two and b's first");
    assert_eq!(queue.place("a", "a1"), Some(1));
  }

  #[test]
  fn a_forecast_is_place_zero_with_a_free_slot_and_ignores_other_groups() {
    let (queue, groups) = queue(
      "[transfers.upload]\nslots = 2\n[transfers.groups.default.upload]\nslots = 1\n\
       [transfers.groups.user_defined.buddies.upload]\nslots = 5\n",
    );
    groups.set("bob", "buddies");
    let empty = queue.forecast("alice");
    assert_eq!(
      (
        empty.group.as_str(),
        empty.total_slots,
        empty.free_slots,
        empty.position
      ),
      (DEFAULT_GROUP, 1, 1, 0)
    );
    let (_, mut bob) = ready(&queue, "bob");
    let (_, mut other) = ready(&queue, "carol");
    assert!(
      started(&mut bob) && started(&mut other),
      "the global slots are taken"
    );
    let buddies = queue.forecast("bob");
    assert_eq!(
      (buddies.total_slots, buddies.free_slots),
      (2, 1),
      "clamped to the global slots; slots of the default group not counted"
    );
    files(&queue, "dave", 3);
    files(&queue, "erin", 1);
    let full = queue.forecast("alice");
    assert_eq!(
      (full.free_slots, full.position),
      (0, 3),
      "carol, dave and erin wait in the group"
    );
  }

  #[test]
  fn a_standing_says_what_an_upload_waits_for() {
    let (queue, _) = queue("[transfers.upload]\nslots = 1\n");
    let (running, mut held) = ready(&queue, "alice");
    assert!(started(&mut held));
    let waiting = files(&queue, "alice", 1)[0];
    let standing = queue.standing(waiting).unwrap();
    assert_eq!(
      standing,
      Standing {
        position: Some(1),
        group: DEFAULT_GROUP.to_string(),
        group_slots_used: 1,
        group_slots: 1,
        global_slots_used: 1,
        global_slots: 1,
        user_active: 1,
      }
    );
    assert_eq!(queue.standing(running).unwrap().position, None);
    assert!(queue.standing(Uuid::new_v4()).is_none());
  }
}
