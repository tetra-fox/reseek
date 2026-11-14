//! Keeps a log line that can repeat every second from flooding the log: the first occurrence is
//! written, then at most one per minute carrying how many were left out in between. A failure
//! that repeats through an outage is warned about once per reason instead.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

const INTERVAL: Duration = Duration::from_secs(60);

/// How many reasons an outage remembers; the oldest is forgotten past it, so a failure whose
/// text differs every time cannot grow the list without end.
const REASONS: usize = 16;

/// Lets a repeating log line through the first time and then at most once a minute. One per
/// call site, usually a `static`.
pub struct LogLimiter {
  state: Mutex<Option<(Instant, u64)>>,
}

impl LogLimiter {
  /// A limiter that has let nothing through yet.
  pub const fn new() -> Self {
    Self {
      state: Mutex::new(None),
    }
  }

  /// `Some(suppressed)` when a line may be written now, with how many were left out since the
  /// last one
  pub fn allow(&self) -> Option<u64> {
    let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
    let now = Instant::now();
    match &mut *state {
      Some((last, suppressed)) if now.duration_since(*last) < INTERVAL => {
        *suppressed += 1;
        None
      }
      Some((last, suppressed)) => {
        *last = now;
        Some(std::mem::take(suppressed))
      }
      None => {
        *state = Some((now, 0));
        Some(0)
      }
    }
  }
}

impl Default for LogLimiter {
  fn default() -> Self {
    Self::new()
  }
}

/// The reasons a failure that repeats through an outage, such as each attempt to reach a server
/// that is down, was warned about: each one once until the outage ends, so two reasons that take
/// turns are not warned about at every attempt. The owner ends the outage, usually once an
/// attempt succeeds.
#[derive(Debug, Default)]
pub struct Outage {
  warned: Vec<String>,
}

impl Outage {
  /// No outage.
  pub const fn new() -> Self {
    Self { warned: Vec::new() }
  }

  /// Whether `reason` is new to the outage, which it then belongs to: a warning tells a new
  /// reason and debug one seen before.
  pub fn new_reason(&mut self, reason: &str) -> bool {
    if self.warned.iter().any(|warned| warned == reason) {
      return false;
    }
    if self.warned.len() == REASONS {
      self.warned.remove(0);
    }
    self.warned.push(reason.to_string());
    true
  }

  /// Whether something failed since the outage began, so an attempt now is a retry.
  pub fn ongoing(&self) -> bool {
    !self.warned.is_empty()
  }

  /// Ends the outage: every reason is new again.
  pub fn end(&mut self) {
    self.warned.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test(start_paused = true)]
  async fn one_line_per_minute_with_the_count_left_out() {
    let limiter = LogLimiter::new();
    assert_eq!(limiter.allow(), Some(0));
    assert_eq!(limiter.allow(), None);
    assert_eq!(limiter.allow(), None);
    tokio::time::sleep(INTERVAL).await;
    assert_eq!(limiter.allow(), Some(2));
    assert_eq!(limiter.allow(), None);
  }

  #[test]
  fn an_outage_warns_about_each_reason_once_until_it_ends() {
    let mut outage = Outage::new();
    assert!(!outage.ongoing());
    let told: Vec<bool> = ["refused", "timed out", "refused", "timed out"]
      .into_iter()
      .map(|reason| outage.new_reason(reason))
      .collect();
    assert_eq!(told, [true, true, false, false]);
    assert!(outage.ongoing());
    outage.end();
    assert!(!outage.ongoing());
    assert!(outage.new_reason("refused"), "a new outage warns again");
  }

  #[test]
  fn an_outage_forgets_its_oldest_reason_past_the_limit() {
    let mut outage = Outage::new();
    for n in 0..=REASONS {
      assert!(outage.new_reason(&n.to_string()));
    }
    assert!(outage.new_reason("0"), "the oldest was forgotten");
    assert!(!outage.new_reason(&REASONS.to_string()));
  }
}
