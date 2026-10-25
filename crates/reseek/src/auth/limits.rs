//! Limits on failed attempts per client address: failed logins and failed API credentials, each
//! counted over a sliding minute and kept in memory only.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use soulseek_client::LogLimiter;
use tokio::time::Instant;
use tracing::warn;

/// How long a failure counts.
pub const WINDOW: Duration = Duration::from_secs(60);

/// Failed logins an address may make in [`WINDOW`].
const LOGINS: usize = 5;

/// Failed token or API key presentations an address may make in [`WINDOW`].
const PRESENTATIONS: usize = 20;

/// The most addresses tracked at once; past it the oldest are forgotten first.
const TRACKED: usize = 65_536;

static TRIPPED: LogLimiter = LogLimiter::new();

/// Both limits.
pub struct Limits {
  /// failed password logins and failed identity provider callbacks
  pub logins: Window,
  /// failed API tokens and API keys
  pub presentations: Window,
}

impl Default for Limits {
  fn default() -> Self {
    Self {
      logins: Window::new("login", LOGINS),
      presentations: Window::new("credential", PRESENTATIONS),
    }
  }
}

/// Failures per address over a sliding window.
pub struct Window {
  what: &'static str,
  limit: usize,
  failures: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl Window {
  fn new(what: &'static str, limit: usize) -> Self {
    Self {
      what,
      limit,
      failures: Mutex::new(HashMap::new()),
    }
  }

  /// Whether `ip` has failed as often as it may within the window; logs when it has.
  pub fn blocked(&self, ip: IpAddr) -> bool {
    let now = Instant::now();
    let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(times) = failures.get_mut(&ip) else {
      return false;
    };
    forget_old(times, now);
    let blocked = times.len() >= self.limit;
    if times.is_empty() {
      failures.remove(&ip);
    }
    if blocked && let Some(suppressed) = TRIPPED.allow() {
      warn!(%ip, kind = self.what, suppressed, "rate limit tripped");
    }
    blocked
  }

  /// Counts a failure of `ip`.
  pub fn failed(&self, ip: IpAddr) {
    let now = Instant::now();
    let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
    if failures.len() >= TRACKED && !failures.contains_key(&ip) {
      failures.retain(|_, times| {
        forget_old(times, now);
        !times.is_empty()
      });
      if failures.len() >= TRACKED {
        let oldest = failures
          .iter()
          .min_by_key(|(_, times)| times.back().copied())
          .map(|(ip, _)| *ip);
        if let Some(oldest) = oldest {
          failures.remove(&oldest);
        }
      }
    }
    let times = failures.entry(ip).or_default();
    forget_old(times, now);
    if times.len() < self.limit {
      times.push_back(now);
    }
  }
}

fn forget_old(times: &mut VecDeque<Instant>, now: Instant) {
  while times
    .front()
    .is_some_and(|at| now.duration_since(*at) >= WINDOW)
  {
    times.pop_front();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test(start_paused = true)]
  async fn an_address_is_blocked_for_a_sliding_minute_after_its_failures() {
    let limits = Limits::default();
    let ip: IpAddr = "192.0.2.1".parse().unwrap();
    let other: IpAddr = "192.0.2.2".parse().unwrap();
    for _ in 0..4 {
      limits.logins.failed(ip);
    }
    assert!(!limits.logins.blocked(ip));
    tokio::time::advance(Duration::from_secs(30)).await;
    limits.logins.failed(ip);
    assert!(limits.logins.blocked(ip));
    assert!(!limits.logins.blocked(other));
    assert!(!limits.presentations.blocked(ip), "the limits are apart");
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(!limits.logins.blocked(ip), "the first four left the window");
    for _ in 0..20 {
      limits.presentations.failed(other);
    }
    assert!(limits.presentations.blocked(other));
  }
}
