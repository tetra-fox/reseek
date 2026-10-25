//! The speed limit shared by every transfer in one direction: a bucket of bytes refilled to its
//! capacity at each 100 ms boundary. A transfer asks for what its next chunk needs and gets what
//! is left, at most one interval's worth; when the bucket is empty it waits for the next boundary.
//! Requesters are served one at a time in arrival order. Bytes granted but not moved are given
//! back, which lets an interval spend up to twice its capacity; the refill drops any surplus.

use std::time::Duration;

use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use crate::error::ClientError;
use crate::lock;

/// how often the bucket is refilled, in milliseconds
const INTERVAL_MILLIS: u64 = 100;
/// how often the bucket is refilled
pub(crate) const INTERVAL: Duration = Duration::from_millis(INTERVAL_MILLIS);

struct State {
  /// the capacity the next refill applies
  capacity: u64,
  /// the capacity of the interval in progress
  current_capacity: u64,
  /// bytes left in this interval
  count: u64,
  /// the interval the bucket was last refilled in, counted from `start`
  interval: u64,
}

pub(crate) struct TokenBucket {
  start: Instant,
  state: std::sync::Mutex<State>,
  /// held by the requester being served, so the others wait in arrival order
  turn: tokio::sync::Mutex<()>,
}

/// Bytes per interval for a limit of `kib_per_second`; 0 means no limit, taken as the largest
/// limit there is
pub(crate) fn capacity(kib_per_second: u32) -> u64 {
  let kib_per_second = if kib_per_second == 0 {
    u32::MAX
  } else {
    kib_per_second
  };
  // at least one byte, so even the lowest limit moves
  (u64::from(kib_per_second) * 1024 / 10).max(1)
}

impl TokenBucket {
  /// a full bucket of `capacity` bytes per interval
  pub(crate) fn new(capacity: u64) -> Self {
    let capacity = capacity.max(1);
    Self {
      start: Instant::now(),
      state: std::sync::Mutex::new(State {
        capacity,
        current_capacity: capacity,
        count: capacity,
        interval: 0,
      }),
      turn: tokio::sync::Mutex::new(()),
    }
  }

  /// the interval `now` falls in
  fn interval_at(&self, now: Instant) -> u64 {
    let elapsed = now.saturating_duration_since(self.start).as_millis();
    u64::try_from(elapsed / u128::from(INTERVAL_MILLIS)).unwrap_or(u64::MAX)
  }

  /// when `interval` starts
  fn boundary(&self, interval: u64) -> Instant {
    self.start + Duration::from_millis(INTERVAL_MILLIS.saturating_mul(interval))
  }

  /// refills the bucket if an interval boundary passed since the last refill
  fn refill(&self, state: &mut State, now: Instant) {
    let interval = self.interval_at(now);
    if interval > state.interval {
      state.interval = interval;
      state.current_capacity = state.capacity;
      state.count = state.current_capacity;
    }
  }

  /// Grants up to `requested` bytes, never more than one interval's capacity: what is left in
  /// the bucket, or what the next interval brings when it is empty. Asking for 0 returns 0 at
  /// once. Fails with `Cancelled`, taking nothing, when `cancel` fires first.
  pub(crate) async fn get(
    &self,
    requested: u32,
    cancel: &CancellationToken,
  ) -> Result<u32, ClientError> {
    if requested == 0 {
      return Ok(0);
    }
    let _turn = tokio::select! {
      biased;
      () = cancel.cancelled() => return Err(ClientError::Cancelled),
      turn = self.turn.lock() => turn,
    };
    loop {
      let next_boundary = {
        let mut state = lock(&self.state);
        let now = Instant::now();
        self.refill(&mut state, now);
        if state.count > 0 {
          let wanted = u64::from(requested).min(state.current_capacity);
          let granted = wanted.min(state.count);
          state.count -= granted;
          tracing::trace!(requested, granted, "speed limit granted");
          return Ok(u32::try_from(granted).expect("at most the u32 that was requested"));
        }
        self.boundary(state.interval + 1)
      };
      tokio::select! {
        biased;
        () = cancel.cancelled() => return Err(ClientError::Cancelled),
        () = sleep_until(next_boundary) => {}
      }
    }
  }

  /// Gives back bytes granted but not moved; the bucket holds at most twice its capacity
  pub(crate) fn give_back(&self, unused: u32) {
    if unused == 0 {
      return;
    }
    let mut state = lock(&self.state);
    self.refill(&mut state, Instant::now());
    let returned = u64::from(unused).min(state.current_capacity);
    state.count = (state.count + returned).min(2 * state.current_capacity);
  }

  /// Changes the capacity from the next interval on; this interval keeps what it has
  pub(crate) fn set_capacity(&self, capacity: u64) {
    let mut state = lock(&self.state);
    state.capacity = capacity.max(1);
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use super::*;

  fn never() -> CancellationToken {
    CancellationToken::new()
  }

  #[test]
  fn capacity_is_a_tenth_of_the_limit_per_second_and_zero_is_unlimited() {
    assert_eq!(capacity(1), 102);
    assert_eq!(capacity(100), 10_240);
    assert_eq!(capacity(0), u64::from(u32::MAX) * 1024 / 10);
  }

  #[tokio::test(start_paused = true)]
  async fn asking_for_nothing_returns_nothing_even_when_empty() {
    let bucket = TokenBucket::new(10);
    assert_eq!(bucket.get(10, &never()).await.unwrap(), 10);
    let before = Instant::now();
    assert_eq!(bucket.get(0, &never()).await.unwrap(), 0);
    assert_eq!(Instant::now(), before, "no wait for a zero request");
  }

  #[tokio::test(start_paused = true)]
  async fn a_request_gets_at_most_one_interval_and_what_is_left() {
    let bucket = TokenBucket::new(100);
    assert_eq!(bucket.get(1000, &never()).await.unwrap(), 100);
    // an empty bucket waits for the next boundary, then grants
    let before = Instant::now();
    assert_eq!(bucket.get(60, &never()).await.unwrap(), 60);
    assert_eq!(Instant::now() - before, INTERVAL);
    // a partial grant of what is left
    assert_eq!(bucket.get(60, &never()).await.unwrap(), 40);
  }

  #[tokio::test(start_paused = true)]
  async fn waits_end_on_interval_boundaries() {
    let bucket = TokenBucket::new(100);
    tokio::time::advance(Duration::from_millis(250)).await;
    assert_eq!(bucket.get(100, &never()).await.unwrap(), 100);
    let before = Instant::now();
    bucket.get(1, &never()).await.unwrap();
    // the boundary at 300 ms, not 100 ms after the last grant
    assert_eq!(Instant::now() - before, Duration::from_millis(50));
  }

  #[tokio::test(start_paused = true)]
  async fn bytes_given_back_burst_to_twice_the_capacity_until_the_refill() {
    let bucket = TokenBucket::new(100);
    assert_eq!(bucket.get(100, &never()).await.unwrap(), 100);
    bucket.give_back(500);
    // at most one interval's worth comes back
    assert_eq!(bucket.get(1000, &never()).await.unwrap(), 100);
    bucket.give_back(100);
    bucket.give_back(100);
    bucket.give_back(100);
    // the count is capped at twice the capacity, a request at the capacity
    assert_eq!(bucket.get(1000, &never()).await.unwrap(), 100);
    assert_eq!(bucket.get(1000, &never()).await.unwrap(), 100);
    // the refill drops the surplus
    bucket.give_back(100);
    tokio::time::advance(INTERVAL).await;
    assert_eq!(bucket.get(1000, &never()).await.unwrap(), 100);
    let before = Instant::now();
    bucket.get(1, &never()).await.unwrap();
    assert_eq!(Instant::now() - before, INTERVAL);
  }

  #[tokio::test(start_paused = true)]
  async fn a_new_capacity_applies_from_the_next_interval() {
    let bucket = TokenBucket::new(100);
    assert_eq!(bucket.get(40, &never()).await.unwrap(), 40);
    bucket.set_capacity(10);
    // this interval keeps its count and its cap
    assert_eq!(bucket.get(1000, &never()).await.unwrap(), 60);
    bucket.set_capacity(1000);
    tokio::time::advance(INTERVAL).await;
    assert_eq!(bucket.get(5000, &never()).await.unwrap(), 1000);
  }

  #[tokio::test(start_paused = true)]
  async fn a_cancelled_waiter_takes_nothing() {
    let bucket = Arc::new(TokenBucket::new(100));
    assert_eq!(bucket.get(100, &never()).await.unwrap(), 100);
    let cancel = CancellationToken::new();
    let waiting = tokio::spawn({
      let bucket = bucket.clone();
      let cancel = cancel.clone();
      async move { bucket.get(50, &cancel).await }
    });
    tokio::task::yield_now().await;
    cancel.cancel();
    assert!(matches!(
      waiting.await.unwrap(),
      Err(ClientError::Cancelled)
    ));
    tokio::time::advance(INTERVAL).await;
    assert_eq!(bucket.get(1000, &never()).await.unwrap(), 100);
  }

  #[tokio::test(start_paused = true)]
  async fn requesters_are_served_in_arrival_order() {
    let bucket = Arc::new(TokenBucket::new(10));
    assert_eq!(bucket.get(10, &never()).await.unwrap(), 10);
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut waiting = Vec::new();
    for name in ["first", "second", "third"] {
      let bucket = bucket.clone();
      let order = order.clone();
      waiting.push(tokio::spawn(async move {
        bucket.get(10, &never()).await.unwrap();
        order.lock().unwrap().push((name, Instant::now()));
      }));
      tokio::task::yield_now().await;
    }
    for task in waiting {
      task.await.unwrap();
    }
    let order = order.lock().unwrap();
    let names: Vec<_> = order.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["first", "second", "third"]);
    // one interval each
    assert_eq!(order[1].1 - order[0].1, INTERVAL);
    assert_eq!(order[2].1 - order[1].1, INTERVAL);
  }
}
