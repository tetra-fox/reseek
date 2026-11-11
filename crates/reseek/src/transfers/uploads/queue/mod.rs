//! The upload queue: which waiting upload starts next. It moves no bytes; it hands out slots,
//! never more than `transfers.upload.slots` at once over every group and never more than a
//! group's own `slots` in that group.
//!
//! An upload is in the queue from when it is accepted until it ends, whether it started or not.
//! It is ready once the client holds the user's one upload at a time for it and asks for a
//! slot. Each pass over the queue starts ready uploads while slots are free: privileged, then
//! the groups in `transfers.groups.order`, and within a group the upload a bump put in front,
//! else the one ready longest for `roundrobin` or queued longest for `firstinfirstout`.
//! A user's group is looked up on every pass, so a user moved between groups waits in their
//! new group; once started, an upload's slot belongs to the group that gave it.
//!
//! Every change to the queue, and the pass it calls for, happens under one lock. Times are a
//! counter taken under that lock, so two uploads are never queued or ready at the same moment.

mod position;

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use reseek_shares::lock;
use tokio::sync::oneshot;
use tracing::{debug, warn};
use uuid::Uuid;

pub use position::Forecast;

use crate::config::enums::QueueStrategy;
use crate::config::{DEFAULT_GROUP, GroupUpload, Groups, Options, PRIVILEGED_GROUP};

/// Looks up the group a user is in, without waiting.
pub type GroupOf = Box<dyn Fn(&str) -> String + Send + Sync>;

/// The upload queue.
pub struct UploadQueue {
  state: Mutex<State>,
  group_of: GroupOf,
}

/// What a bump did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bumped {
  /// the upload waits in front of its group until the group next starts an upload
  Waiting,
  /// the upload started already, or the queue does not hold it
  NotWaiting,
}

/// A slot the queue was asked for and cannot give.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("upload {0} is not in the queue or asked for its slot already")]
pub struct NotQueued(pub Uuid);

struct State {
  global_slots: u32,
  /// privileged, then in the configured order
  groups: Vec<Group>,
  /// in the order each user first queued
  users: Vec<UserEntries>,
  /// what the groups were built from
  built_from: (Groups, u32),
  /// the counter times are taken from
  clock: u64,
  /// group names users were found in that the queue has no group for, logged once each
  unknown: HashSet<String>,
}

struct Group {
  name: String,
  strategy: QueueStrategy,
  /// at most the global slots
  slots: u32,
  /// the uploads holding one of the group's slots
  used: BTreeSet<Uuid>,
}

struct UserEntries {
  username: String,
  /// in the order queued
  entries: Vec<Entry>,
}

struct Entry {
  id: Uuid,
  filename: String,
  queued: u64,
  ready: Option<u64>,
  started: Option<u64>,
  /// the group that gave the slot
  group: Option<String>,
  bumped: Option<u64>,
  /// fired when the slot is given
  start: Option<oneshot::Sender<()>>,
  /// taken by the upload when it asks for its slot
  started_signal: Option<oneshot::Receiver<()>>,
}

impl State {
  fn tick(&mut self) -> u64 {
    self.clock += 1;
    self.clock
  }

  fn group(&self, name: &str) -> Option<&Group> {
    self.groups.iter().find(|g| g.name == name)
  }

  fn used(&self) -> u32 {
    self.groups.iter().map(|g| g.used.len() as u32).sum()
  }

  /// The user and the index in their entries of the upload `id`.
  fn find(&self, id: Uuid) -> Option<(usize, usize)> {
    self
      .users
      .iter()
      .enumerate()
      .find_map(|(u, user)| user.entries.iter().position(|e| e.id == id).map(|i| (u, i)))
  }
}

/// A group's table entry from its options: at most `global` slots, `0` or none being all of
/// them, and the queue order of `everyone` when it sets none.
fn group(name: &str, upload: &GroupUpload, everyone: &GroupUpload, global: u32) -> Group {
  Group {
    name: name.to_string(),
    strategy: upload.strategy.or(everyone.strategy).unwrap_or_default(),
    slots: upload
      .slots
      .filter(|&slots| slots != 0)
      .map_or(global, |slots| slots.min(global)),
    used: BTreeSet::new(),
  }
}

/// The queues `options` call for: `privileged` first in request order with every slot, then
/// each group with a queue of its own, highest first, then default.
fn groups(options: &Options) -> Vec<Group> {
  let global = options.transfers.upload.slots;
  let configured = &options.transfers.groups;
  let everyone = &configured.default.upload;
  let mut groups = vec![Group {
    name: PRIVILEGED_GROUP.to_string(),
    strategy: QueueStrategy::FirstInFirstOut,
    slots: global,
    used: BTreeSet::new(),
  }];
  for name in configured.ordered() {
    if let Some(upload) = configured.upload(name).filter(|upload| upload.own_queue()) {
      groups.push(group(name, upload, everyone, global));
    }
  }
  groups.push(group(DEFAULT_GROUP, everyone, everyone, global));
  groups
}

/// The key a group picks its next upload by, lowest first: the latest bump, then the time the
/// strategy goes by, then the other time, then the id.
fn pick_key(entry: &Entry, strategy: QueueStrategy) -> (Reverse<Option<u64>>, u64, u64, Uuid) {
  let ready = entry.ready.unwrap_or(u64::MAX);
  let (first, second) = match strategy {
    QueueStrategy::RoundRobin => (ready, entry.queued),
    QueueStrategy::FirstInFirstOut => (entry.queued, ready),
  };
  (Reverse(entry.bumped), first, second, entry.id)
}

impl UploadQueue {
  /// An empty queue with the groups of `options`, looking up users' groups with `group_of`.
  pub fn new(options: &Options, group_of: GroupOf) -> Self {
    let transfers = &options.transfers;
    Self {
      state: Mutex::new(State {
        global_slots: transfers.upload.slots,
        groups: groups(options),
        users: Vec::new(),
        built_from: (transfers.groups.clone(), transfers.upload.slots),
        clock: 0,
        unknown: HashSet::new(),
      }),
      group_of,
    }
  }

  fn lock(&self) -> MutexGuard<'_, State> {
    lock(&self.state)
  }

  /// The group `username` is queued in now: their group, or the default group when the queue
  /// has none of that name, which is logged once per name.
  fn queued_group(&self, state: &mut State, username: &str) -> String {
    let name = (self.group_of)(username);
    if state.group(&name).is_some() {
      return name;
    }
    if state.unknown.insert(name.clone()) {
      warn!(
        group = name,
        "users in an upload group the queue does not have wait in the default group"
      );
    }
    DEFAULT_GROUP.to_string()
  }

  /// Builds the groups again from `options` when the groups or the global slots changed,
  /// keeping the slots each group of the same name holds, then starts what that allows. Slots
  /// held by a group that is gone are no longer counted.
  pub fn configure(&self, options: &Options) {
    let mut state = self.lock();
    let transfers = &options.transfers;
    let wanted = (transfers.groups.clone(), transfers.upload.slots);
    if state.built_from == wanted {
      return;
    }
    let mut rebuilt = groups(options);
    for group in &mut rebuilt {
      if let Some(old) = state.groups.iter_mut().find(|g| g.name == group.name) {
        group.used = std::mem::take(&mut old.used);
      }
    }
    state.groups = rebuilt;
    state.global_slots = transfers.upload.slots;
    state.built_from = wanted;
    state.unknown.clear();
    debug!(
      global_slots = state.global_slots,
      groups = state.groups.len(),
      "upload groups configured"
    );
    self.process(&mut state);
  }

  /// Puts the upload `id` of `filename` to `username` at the back of the queue.
  pub fn enqueue(&self, id: Uuid, username: &str, filename: &str) {
    let mut state = self.lock();
    let queued = state.tick();
    let (start, started_signal) = oneshot::channel();
    let entry = Entry {
      id,
      filename: filename.to_string(),
      queued,
      ready: None,
      started: None,
      group: None,
      bumped: None,
      start: Some(start),
      started_signal: Some(started_signal),
    };
    match state.users.iter_mut().find(|u| u.username == username) {
      Some(user) => user.entries.push(entry),
      None => state.users.push(UserEntries {
        username: username.to_string(),
        entries: vec![entry],
      }),
    }
    debug!(%id, username, filename, "upload queued");
    self.process(&mut state);
  }

  /// Marks the upload `id` ready and returns what fires once it is given a slot, which may be
  /// at once. Asked once per upload.
  pub fn await_start(&self, id: Uuid) -> Result<oneshot::Receiver<()>, NotQueued> {
    let mut state = self.lock();
    let ready = state.tick();
    let (u, i) = state.find(id).ok_or(NotQueued(id))?;
    let entry = &mut state.users[u].entries[i];
    let signal = entry.started_signal.take().ok_or(NotQueued(id))?;
    entry.ready = Some(ready);
    debug!(%id, username = state.users[u].username, "upload ready for a slot");
    self.process(&mut state);
    Ok(signal)
  }

  /// Takes the upload `id` out of the queue: a slot it holds goes back to the group that gave
  /// it, and one it waits for is given up. Returns whether the queue held it, so a second call
  /// for an upload changes nothing.
  pub fn complete(&self, id: Uuid) -> bool {
    let mut state = self.lock();
    let Some((u, i)) = state.find(id) else {
      return false;
    };
    let entry = state.users[u].entries.remove(i);
    let username = state.users[u].username.clone();
    if state.users[u].entries.is_empty() {
      state.users.remove(u);
    }
    if entry.started.is_none() {
      debug!(%id, username, filename = entry.filename, "upload left the queue without starting");
    }
    if let Some(name) = &entry.group
      && let Some(group) = state.groups.iter_mut().find(|g| &g.name == name)
    {
      group.used.remove(&id);
      debug!(
        group = group.name,
        used = group.used.len(),
        slots = group.slots,
        "upload slot freed"
      );
    }
    debug!(%id, username, "upload out of the queue");
    self.process(&mut state);
    true
  }

  /// Puts the waiting upload `id` in front of its group until the group next starts an
  /// upload.
  pub fn bump(&self, id: Uuid) -> Bumped {
    let mut state = self.lock();
    let bumped = state.tick();
    let Some((u, i)) = state.find(id) else {
      return Bumped::NotWaiting;
    };
    let entry = &mut state.users[u].entries[i];
    if entry.started.is_some() {
      return Bumped::NotWaiting;
    }
    entry.bumped = Some(bumped);
    debug!(%id, "upload bumped to the front of its group");
    self.process(&mut state);
    Bumped::Waiting
  }

  /// The uploads to `username` waiting for a slot, in the order queued.
  pub fn waiting(&self, username: &str) -> Vec<Uuid> {
    let state = self.lock();
    state
      .users
      .iter()
      .filter(|u| u.username == username)
      .flat_map(|u| {
        u.entries
          .iter()
          .filter(|e| e.started.is_none())
          .map(|e| e.id)
      })
      .collect()
  }

  /// The users with an upload in the queue.
  pub fn users(&self) -> Vec<String> {
    self
      .lock()
      .users
      .iter()
      .map(|u| u.username.clone())
      .collect()
  }

  /// Starts ready uploads while slots are free, one grant at a time, each looking at the queue
  /// as the last left it.
  fn process(&self, state: &mut State) {
    loop {
      if state.used() >= state.global_slots {
        return;
      }
      let mut ready: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
      for u in 0..state.users.len() {
        let waiting: Vec<usize> = state.users[u]
          .entries
          .iter()
          .enumerate()
          .filter(|(_, e)| e.ready.is_some() && e.started.is_none())
          .map(|(i, _)| i)
          .collect();
        if waiting.is_empty() {
          continue;
        }
        let username = state.users[u].username.clone();
        let group = self.queued_group(state, &username);
        ready
          .entry(group)
          .or_default()
          .extend(waiting.into_iter().map(|i| (u, i)));
      }
      let picked = state.groups.iter().enumerate().find_map(|(g, group)| {
        if group.used.len() as u32 >= group.slots {
          return None;
        }
        let candidates = ready.get(&group.name)?;
        candidates
          .iter()
          .min_by_key(|(u, i)| pick_key(&state.users[*u].entries[*i], group.strategy))
          .map(|&(u, i)| (g, u, i))
      });
      let Some((g, u, i)) = picked else {
        return;
      };
      self.grant(state, g, u, i);
    }
  }

  /// Gives the upload at `i` of user `u` a slot of group `g`, and spends the bumps of the
  /// group's other waiting uploads. An upload that stopped waiting meanwhile is not ready any
  /// more instead.
  fn grant(&self, state: &mut State, g: usize, u: usize, i: usize) {
    let started = state.tick();
    let group_name = state.groups[g].name.clone();
    let username = state.users[u].username.clone();
    let entry = &mut state.users[u].entries[i];
    let id = entry.id;
    let fired = entry
      .start
      .take()
      .is_some_and(|start| start.send(()).is_ok());
    if !fired {
      entry.ready = None;
      debug!(%id, username, "upload stopped waiting before its slot; passed over");
      return;
    }
    entry.started = Some(started);
    entry.group = Some(group_name.clone());
    entry.bumped = None;
    let group = &mut state.groups[g];
    group.used.insert(id);
    debug!(
      %id,
      username,
      group = group.name,
      used = group.used.len(),
      slots = group.slots,
      "upload slot given"
    );
    let users: Vec<String> = state.users.iter().map(|u| u.username.clone()).collect();
    for (u, username) in users.iter().enumerate() {
      if self.queued_group(state, username) != group_name {
        continue;
      }
      for entry in &mut state.users[u].entries {
        entry.bumped = None;
      }
    }
  }
}

#[cfg(test)]
pub(super) mod tests {
  use std::sync::Arc;

  use super::*;

  /// Users' groups a test sets as it goes.
  #[derive(Clone, Default)]
  pub struct Groups(Arc<Mutex<HashMap<String, String>>>);

  impl Groups {
    pub fn set(&self, username: &str, group: &str) {
      self
        .0
        .lock()
        .unwrap()
        .insert(username.to_string(), group.to_string());
    }

    pub fn of(&self) -> GroupOf {
      let groups = self.0.clone();
      Box::new(move |username| {
        groups
          .lock()
          .unwrap()
          .get(username)
          .cloned()
          .unwrap_or_else(|| DEFAULT_GROUP.to_string())
      })
    }
  }

  pub fn options(toml: &str) -> Options {
    toml::from_str(toml).unwrap()
  }

  /// A queue for `toml`'s options, with the groups a test sets.
  pub fn queue(toml: &str) -> (UploadQueue, Groups) {
    let groups = Groups::default();
    (UploadQueue::new(&options(toml), groups.of()), groups)
  }

  /// Queues an upload for `username` and asks for its slot; returns its id and its start.
  pub fn ready(queue: &UploadQueue, username: &str) -> (Uuid, oneshot::Receiver<()>) {
    let id = Uuid::new_v4();
    queue.enqueue(id, username, "file");
    let start = queue.await_start(id).unwrap();
    (id, start)
  }

  pub fn started(start: &mut oneshot::Receiver<()>) -> bool {
    start.try_recv().is_ok()
  }

  #[test]
  fn nothing_starts_past_the_global_slots_whatever_a_group_allows() {
    let (queue, _) = queue("[transfers.upload]\nslots = 2\n");
    let (_, mut a) = ready(&queue, "alice");
    let (_, mut b) = ready(&queue, "bob");
    let (_, mut c) = ready(&queue, "carol");
    assert!(started(&mut a) && started(&mut b));
    assert!(
      !started(&mut c),
      "the default group has room, the global slots do not"
    );
  }

  #[test]
  fn a_higher_group_waiting_takes_the_next_free_slot() {
    let (queue, groups) = queue(
      "[transfers.upload]\nslots = 1\n\
       [transfers.groups.buddies.upload]\nslots = 1\n",
    );
    groups.set("bob", "buddies");
    let (running, mut first) = ready(&queue, "erin");
    assert!(started(&mut first));
    let (_, mut carol) = ready(&queue, "carol");
    let (_, mut bob) = ready(&queue, "bob");
    queue.complete(running);
    assert!(started(&mut bob), "the buddies first");
    assert!(!started(&mut carol));
  }

  #[test]
  fn the_order_decides_which_group_takes_the_next_free_slot() {
    for (order, first) in [("vip", "bob"), ("friends", "carol")] {
      let (queue, groups) = queue(&format!(
        "[transfers.upload]\nslots = 1\n\
         [transfers.groups]\norder = [\"{order}\", \"buddies\", \"leechers\"]\n\
         [transfers.groups.user_defined.vip.upload]\nslots = 1\n\
         [transfers.groups.user_defined.friends.upload]\nslots = 1\n"
      ));
      groups.set("bob", "vip");
      groups.set("carol", "friends");
      let (running, mut held) = ready(&queue, "erin");
      assert!(started(&mut held));
      let (_, mut carol) = ready(&queue, "carol");
      let (_, mut bob) = ready(&queue, "bob");
      queue.complete(running);
      let picked = if started(&mut bob) {
        "bob"
      } else if started(&mut carol) {
        "carol"
      } else {
        "nobody"
      };
      assert_eq!(picked, first, "{order} first");
    }
  }

  #[test]
  fn a_capped_group_lets_a_lower_one_take_the_slot() {
    let (queue, groups) = queue(
      "[transfers.upload]\nslots = 3\n\
       [transfers.groups.buddies.upload]\nslots = 1\n",
    );
    groups.set("bob", "buddies");
    groups.set("dave", "buddies");
    let (_, mut bob) = ready(&queue, "bob");
    let (_, mut dave) = ready(&queue, "dave");
    let (_, mut carol) = ready(&queue, "carol");
    assert!(started(&mut bob));
    assert!(!started(&mut dave), "the buddies' one slot is taken");
    assert!(started(&mut carol), "so the default group gets the next");
  }

  #[test]
  fn round_robin_serves_the_upload_ready_longest_and_fifo_the_one_queued_longest() {
    for (strategy, first) in [("roundrobin", "bob"), ("firstinfirstout", "alice")] {
      let (queue, _) = queue(&format!(
        "[transfers.upload]\nslots = 1\n[transfers.groups.default.upload]\nstrategy = \"{strategy}\"\n"
      ));
      let (running, mut held) = ready(&queue, "erin");
      assert!(started(&mut held));
      // alice queued first, bob was ready first
      let alice = Uuid::new_v4();
      queue.enqueue(alice, "alice", "file");
      let (_, mut bob) = ready(&queue, "bob");
      let mut alice = queue.await_start(alice).unwrap();
      queue.complete(running);
      let picked = if started(&mut alice) {
        "alice"
      } else if started(&mut bob) {
        "bob"
      } else {
        "nobody"
      };
      assert_eq!(picked, first, "{strategy}");
    }
  }

  #[test]
  fn a_slot_is_given_back_to_the_group_that_gave_it_after_the_user_moves() {
    let (queue, groups) = queue(
      "[transfers.upload]\nslots = 4\n\
       [transfers.groups.buddies.upload]\nslots = 1\n",
    );
    groups.set("bob", "buddies");
    let (id, mut bob) = ready(&queue, "bob");
    assert!(started(&mut bob));
    groups.set("bob", DEFAULT_GROUP);
    groups.set("dave", "buddies");
    let (_, mut dave) = ready(&queue, "dave");
    assert!(
      !started(&mut dave),
      "bob's slot still counts for the buddies"
    );
    assert!(queue.complete(id));
    assert!(started(&mut dave), "and goes back to them");
    assert!(!queue.complete(id), "once");
  }

  #[test]
  fn completion_failure_and_cancel_each_free_their_slot_once() {
    let (queue, _) = queue("[transfers.upload]\nslots = 1\n");
    let (done, mut first) = ready(&queue, "alice");
    let (failed, mut second) = ready(&queue, "bob");
    let (_, mut third) = ready(&queue, "carol");
    assert!(started(&mut first));
    // a waiting upload that is canceled gives up its wait and frees nothing
    let canceled = Uuid::new_v4();
    queue.enqueue(canceled, "dave", "file");
    let waiting = queue.await_start(canceled).unwrap();
    drop(waiting);
    assert!(queue.complete(canceled));
    assert!(!started(&mut second));
    assert!(queue.complete(done), "the upload that succeeded");
    assert!(started(&mut second));
    assert!(queue.complete(failed), "the upload that failed");
    assert!(started(&mut third));
    assert!(!queue.complete(done) && !queue.complete(failed));
  }

  #[test]
  fn an_upload_that_stopped_waiting_is_passed_over() {
    let (queue, _) = queue("[transfers.upload]\nslots = 1\n");
    let (running, mut held) = ready(&queue, "erin");
    assert!(started(&mut held));
    let (gone, signal) = ready(&queue, "alice");
    drop(signal);
    let (_, mut bob) = ready(&queue, "bob");
    queue.complete(running);
    assert!(started(&mut bob));
    assert!(queue.complete(gone));
  }

  #[test]
  fn more_slots_start_every_upload_they_allow_at_once() {
    let (queue, _) = queue("[transfers.upload]\nslots = 1\n");
    let mut waiting: Vec<_> = ["a", "b", "c", "d"]
      .iter()
      .map(|user| ready(&queue, user).1)
      .collect();
    let starts = |waiting: &mut Vec<oneshot::Receiver<()>>| {
      waiting.iter_mut().map(started).filter(|&s| s).count()
    };
    assert_eq!(starts(&mut waiting), 1);
    queue.configure(&options("[transfers.upload]\nslots = 3\n"));
    assert_eq!(starts(&mut waiting), 2, "two more start in one pass");
  }

  #[test]
  fn a_reconfigured_group_keeps_its_slots_and_slots_never_pass_the_global_ones() {
    let (queue, _) =
      queue("[transfers.upload]\nslots = 3\n[transfers.groups.default.upload]\nslots = 10\n");
    let (_, mut a) = ready(&queue, "a");
    let (_, mut b) = ready(&queue, "b");
    assert!(started(&mut a) && started(&mut b));
    queue.configure(&options(
      "[transfers.upload]\nslots = 3\n[transfers.groups.default.upload]\nslots = 2\n",
    ));
    let (_, mut c) = ready(&queue, "c");
    assert!(
      !started(&mut c),
      "the two running still hold the default group's slots"
    );
    let state = queue.lock();
    assert_eq!(state.group(DEFAULT_GROUP).unwrap().used.len(), 2);
    assert_eq!(state.group(PRIVILEGED_GROUP).unwrap().slots, 3);
  }

  #[test]
  fn a_bump_puts_an_upload_first_for_its_groups_next_slot_only() {
    let (queue, _) = queue("[transfers.upload]\nslots = 1\n");
    let (running, mut held) = ready(&queue, "erin");
    assert!(started(&mut held));
    let (_, mut alice) = ready(&queue, "alice");
    let (carol, mut carol_start) = ready(&queue, "carol");
    assert_eq!(queue.bump(carol), Bumped::Waiting);
    queue.complete(running);
    assert!(started(&mut carol_start), "the bumped upload goes first");
    assert!(!started(&mut alice));
    assert_eq!(queue.bump(carol), Bumped::NotWaiting, "started already");
    assert_eq!(queue.bump(Uuid::new_v4()), Bumped::NotWaiting);

    // a bump not used by the group's next slot is spent with it
    let (_, mut bob) = ready(&queue, "bob");
    let dave = Uuid::new_v4();
    queue.enqueue(dave, "dave", "file");
    assert_eq!(queue.bump(dave), Bumped::Waiting, "waiting, not ready yet");
    queue.complete(carol);
    assert!(started(&mut alice), "dave was not ready for this slot");
    let mut dave_start = queue.await_start(dave).unwrap();
    queue.complete(queue_running(&queue, "alice"));
    assert!(
      started(&mut bob),
      "bob was ready longer, and dave's bump is spent"
    );
    assert!(!started(&mut dave_start));
  }

  /// The id of the upload of `username` holding a slot.
  fn queue_running(queue: &UploadQueue, username: &str) -> Uuid {
    let state = queue.lock();
    let user = state.users.iter().find(|u| u.username == username).unwrap();
    user
      .entries
      .iter()
      .find(|e| e.started.is_some())
      .unwrap()
      .id
  }

  #[test]
  fn a_user_in_a_group_the_queue_does_not_have_waits_in_the_default_group() {
    let (queue, groups) = queue("[transfers.upload]\nslots = 1\n");
    groups.set("alice", "gone");
    let (id, mut alice) = ready(&queue, "alice");
    assert!(started(&mut alice));
    let state = queue.lock();
    assert!(state.group(DEFAULT_GROUP).unwrap().used.contains(&id));
  }
}
