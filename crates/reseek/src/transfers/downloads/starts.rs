//! The local queue of downloads: the order downloads waiting here start in, which the user can
//! change. A download waits for two things, both granted in queue order: its request's turn,
//! held until its peer has queued it, so a big request does not ask a peer for dozens of files
//! at once, and a download slot while `transfers.download.slots` is set, held for each try. A
//! download that cannot have its request's turn yet lets the next one in the queue start. Each
//! download's place comes with its record, and the places a reorder gives are written back, so
//! the order outlives a restart.
//!
//! A user's downloads are asked of them in the order they were granted: the client takes a
//! download's place among its user's downloads when it is first polled, so each waits for the
//! ones of its user granted before it to be polled first.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use reseek_shares::lock;
use tokio::sync::{Notify, oneshot};
use uuid::Uuid;

/// What a waiting download waits for.
struct Waiting {
  id: Uuid,
  /// the user the download is from
  username: String,
  /// the request whose turn it needs, with how many of its files may hold one at once; none
  /// on a retry that holds its turn already
  request: Option<(u64, usize)>,
  granted: oneshot::Sender<Grant>,
}

#[derive(Default)]
struct State {
  /// where each download is in the queue; lower starts first
  positions: HashMap<Uuid, f64>,
  /// the downloads waiting, in queue order
  waiting: Vec<Waiting>,
  /// the slots held by downloads trying now
  slots: usize,
  /// the slots there are, none for no limit
  limit: Option<usize>,
  /// how many turns each request with any holds
  held: HashMap<u64, usize>,
  next_request: u64,
  /// the user of each download granted and not yet asked of its peer, by the order it was
  /// granted in
  asking: BTreeMap<u64, String>,
  next_ask: u64,
}

/// The local queue of downloads.
#[derive(Clone, Default)]
pub(super) struct Starts {
  state: Arc<Mutex<State>>,
  /// woken whenever a granted download was asked of its peer, or will not be
  asked: Arc<Notify>,
}

/// What a download was granted: a slot for one try, its request's turn when it waited for one,
/// and its place among its user's downloads to be asked of the peer. Each is given back when
/// dropped.
pub(super) struct Grant {
  pub slot: Slot,
  pub turn: Option<Turn>,
  pub ask: Ask,
}

/// A granted download's place among its user's downloads to be asked of the peer, held until
/// it was.
pub(super) struct Ask {
  starts: Starts,
  number: u64,
}

/// A download slot, held for one try.
pub(super) struct Slot {
  starts: Starts,
}

/// A request's turn, held until the peer queued the download.
pub(super) struct Turn {
  starts: Starts,
  request: u64,
}

impl Drop for Slot {
  fn drop(&mut self) {
    let mut state = lock(&self.starts.state);
    state.slots = state.slots.saturating_sub(1);
    self.starts.grant(&mut state);
  }
}

impl Drop for Turn {
  fn drop(&mut self) {
    let mut state = lock(&self.starts.state);
    if let Some(held) = state.held.get_mut(&self.request) {
      *held = held.saturating_sub(1);
      if *held == 0 {
        state.held.remove(&self.request);
      }
    }
    self.starts.grant(&mut state);
  }
}

impl Drop for Ask {
  fn drop(&mut self) {
    lock(&self.starts.state).asking.remove(&self.number);
    self.starts.asked.notify_waiters();
  }
}

impl Ask {
  /// Waits until every download of the same user granted before this one was asked of the
  /// peer or will not be.
  pub async fn ready(&self) {
    loop {
      let asked = self.starts.asked.notified();
      tokio::pin!(asked);
      // registered before the check, so a wake between the check and the wait is not missed
      asked.as_mut().enable();
      if self.first() {
        return;
      }
      asked.await;
    }
  }

  /// Whether no download of the same user granted before this one waits to be asked.
  fn first(&self) -> bool {
    let state = lock(&self.starts.state);
    let username = &state.asking[&self.number];
    !state
      .asking
      .range(..self.number)
      .any(|(_, earlier)| earlier == username)
  }
}

impl Starts {
  /// Grants every waiting download what it waits for, in queue order, while there are slots;
  /// `state` is this queue's, locked.
  fn grant(&self, state: &mut State) {
    let mut at = 0;
    while at < state.waiting.len() {
      if state.limit.is_some_and(|limit| state.slots >= limit) {
        break;
      }
      let ready = match state.waiting[at].request {
        Some((request, limit)) => state.held.get(&request).copied().unwrap_or(0) < limit,
        None => true,
      };
      if !ready {
        at += 1;
        continue;
      }
      let waiting = state.waiting.remove(at);
      let number = state.next_ask;
      let grant = Grant {
        slot: Slot {
          starts: self.clone(),
        },
        turn: waiting.request.map(|(request, _)| Turn {
          starts: self.clone(),
          request,
        }),
        ask: Ask {
          starts: self.clone(),
          number,
        },
      };
      if let Err(unwanted) = waiting.granted.send(grant) {
        // the waiter left. Nothing was counted for these handles yet, and dropping them would
        // give back what they never took and lock the state held here
        std::mem::forget(unwanted);
        continue;
      }
      if let Some((request, _)) = waiting.request {
        *state.held.entry(request).or_default() += 1;
      }
      state.slots += 1;
      state.next_ask += 1;
      state.asking.insert(number, waiting.username);
    }
  }

  /// A new request, whose files share their turns.
  pub fn request(&self) -> u64 {
    let mut state = lock(&self.state);
    state.next_request += 1;
    state.next_request
  }

  /// Takes the download `id` out of the queue for good.
  pub fn forget(&self, id: Uuid) {
    let mut state = lock(&self.state);
    state.positions.remove(&id);
    state.waiting.retain(|waiting| waiting.id != id);
  }

  /// Sets how many slots there are, none for no limit, and grants what that frees.
  pub fn set_limit(&self, limit: Option<usize>) {
    let mut state = lock(&self.state);
    state.limit = limit;
    self.grant(&mut state);
  }

  /// Puts the download `id` of `username` in the queue at `position`, or at the place a
  /// reorder gave it since it first waited, at once; the future waits for its turn there: a
  /// slot, and a turn of `request` when given, whose files may hold that many at once.
  pub fn wait(
    &self,
    id: Uuid,
    username: &str,
    position: f64,
    request: Option<(u64, usize)>,
  ) -> impl Future<Output = Grant> + Send + use<> {
    let (granted, grant_rx) = oneshot::channel();
    {
      let mut state = lock(&self.state);
      let position = *state.positions.entry(id).or_insert(position);
      let at = state
        .waiting
        .iter()
        .position(|w| state.positions.get(&w.id).is_some_and(|p| *p > position))
        .unwrap_or(state.waiting.len());
      state.waiting.insert(
        at,
        Waiting {
          id,
          username: username.to_string(),
          request,
          granted,
        },
      );
      self.grant(&mut state);
    }
    async move {
      grant_rx
        .await
        .expect("a waiter's sender stays in the queue until it is granted")
    }
  }

  /// The downloads waiting, in queue order.
  pub fn order(&self) -> Vec<Uuid> {
    lock(&self.state).waiting.iter().map(|w| w.id).collect()
  }

  /// The downloads waiting, each with its place.
  pub fn placed(&self) -> Vec<(Uuid, f64)> {
    let state = lock(&self.state);
    state
      .waiting
      .iter()
      .map(|w| (w.id, state.positions[&w.id]))
      .collect()
  }

  /// Moves the waiting downloads `ids`, in the order given, to just before `before`, or to the
  /// end of the queue when it is none or not waiting. Downloads that are not waiting are left
  /// out; returns those moved.
  pub fn move_to(&self, ids: &[Uuid], before: Option<Uuid>) -> Vec<Uuid> {
    let mut state = lock(&self.state);
    let waiting: Vec<Uuid> = state.waiting.iter().map(|w| w.id).collect();
    let moved: Vec<Uuid> = ids
      .iter()
      .copied()
      .filter(|id| waiting.contains(id) && Some(*id) != before)
      .collect();
    if moved.is_empty() {
      return moved;
    }
    let rest: Vec<Uuid> = waiting
      .iter()
      .copied()
      .filter(|id| !moved.contains(id))
      .collect();
    let at = before
      .and_then(|before| rest.iter().position(|id| *id == before))
      .unwrap_or(rest.len());
    let mut order = rest[..at].to_vec();
    order.extend(&moved);
    order.extend(&rest[at..]);
    // positions run in the new order below the first waiting one's, so a download not waiting
    // now keeps its place against them
    let base = waiting
      .first()
      .and_then(|id| state.positions.get(id).copied())
      .unwrap_or(0.0);
    let step = 1.0 / (order.len() as f64 + 1.0);
    for (n, id) in order.iter().enumerate() {
      state.positions.insert(*id, base + n as f64 * step);
    }
    let mut taken: Vec<Waiting> = std::mem::take(&mut state.waiting);
    taken.sort_by(|a, b| {
      let at = |w: &Waiting| state.positions.get(&w.id).copied().unwrap_or(f64::MAX);
      at(a).total_cmp(&at(b))
    });
    state.waiting = taken;
    self.grant(&mut state);
    moved
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;

  /// Starts `id` waiting in `queue` with a turn of `request` when given, and hands back its
  /// grant once it has one.
  fn waiter(
    queue: &Starts,
    id: Uuid,
    request: Option<(u64, usize)>,
  ) -> tokio::task::JoinHandle<Grant> {
    let queue = queue.clone();
    let position = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as f64;
    tokio::spawn(async move { queue.wait(id, "alice", position, request).await })
  }

  /// The places the waiters take, in the order they are made.
  static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

  async fn granted(waiter: &mut tokio::task::JoinHandle<Grant>) -> Option<Grant> {
    tokio::time::timeout(Duration::from_millis(50), waiter)
      .await
      .ok()
      .map(Result::unwrap)
  }

  #[tokio::test]
  async fn waiting_downloads_start_in_the_order_the_user_set() {
    let queue = Starts::default();
    queue.set_limit(Some(1));
    let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
    let mut first = waiter(&queue, ids[0], None);
    let running = granted(&mut first)
      .await
      .expect("a free slot is granted at once");
    let mut waiters: Vec<_> = Vec::new();
    for id in &ids[1..] {
      waiters.push(waiter(&queue, *id, None));
      tokio::task::yield_now().await;
    }
    while queue.order().len() < 3 {
      tokio::task::yield_now().await;
    }
    assert_eq!(queue.order(), ids[1..]);
    assert_eq!(queue.move_to(&[ids[3]], Some(ids[1])), [ids[3]]);
    assert_eq!(queue.order(), [ids[3], ids[1], ids[2]]);
    let placed = queue.placed();
    assert!(
      placed.windows(2).all(|pair| pair[0].1 < pair[1].1),
      "the places written back hold the new order: {placed:?}"
    );
    assert!(
      queue.move_to(&[ids[0]], None).is_empty(),
      "a running one is not waiting"
    );

    drop(running);
    let next = granted(&mut waiters[2])
      .await
      .expect("the one moved first starts first");
    assert!(granted(&mut waiters[0]).await.is_none(), "one slot");
    drop(next);
    assert!(granted(&mut waiters[0]).await.is_some());
  }

  #[tokio::test]
  async fn a_request_holding_its_turns_lets_another_requests_file_start() {
    let queue = Starts::default();
    let (big, small) = (queue.request(), queue.request());
    let mut first = waiter(&queue, Uuid::new_v4(), Some((big, 1)));
    let held = granted(&mut first).await.unwrap();
    assert!(held.turn.is_some());
    let mut second = waiter(&queue, Uuid::new_v4(), Some((big, 1)));
    let mut other = waiter(&queue, Uuid::new_v4(), Some((small, 1)));
    assert!(
      granted(&mut other).await.is_some(),
      "another request goes past it"
    );
    assert!(
      granted(&mut second).await.is_none(),
      "its request's one turn is held"
    );
    // the peer queued the first: its turn goes and the next file of the request starts
    drop(held.turn);
    assert!(granted(&mut second).await.is_some());
    drop(held.slot);
  }
}
