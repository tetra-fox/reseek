//! Limits on failed attempts per client address: failed logins and failed API credentials, each
//! counted over a sliding minute and kept in memory only. An IPv6 client is counted by its /64,
//! the block one host is usually given, so it cannot spread attempts over its own addresses. A
//! password login is counted when it starts and forgiven when it succeeds, so attempts made at
//! the same time are held to the limit too.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;
use std::time::Duration;

use soulseek_client::LogLimiter;
use tokio::time::Instant;
use tracing::warn;

use crate::sync::lock;

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
    let key = counted_as(ip);
    let mut failures = lock(&self.failures);
    let Some(times) = failures.get_mut(&key) else {
      return false;
    };
    forget_old(times, now);
    let blocked = times.len() >= self.limit;
    if times.is_empty() {
      failures.remove(&key);
    }
    if blocked && let Some(suppressed) = TRIPPED.allow() {
      warn!(%ip, kind = self.what, suppressed, "rate limit tripped");
    }
    blocked
  }

  /// Counts a failure of `ip`.
  pub fn failed(&self, ip: IpAddr) {
    let now = Instant::now();
    let mut failures = lock(&self.failures);
    let times = tracked(&mut failures, counted_as(ip), now);
    if times.len() < self.limit {
      times.push_back(now);
    }
  }

  /// Starts an attempt of `ip`, counted as a failure until it is forgiven; `None` when `ip` has
  /// failed or is trying as often as it may within the window, which is logged.
  pub fn attempt(&self, ip: IpAddr) -> Option<Attempt> {
    let now = Instant::now();
    let key = counted_as(ip);
    let mut failures = lock(&self.failures);
    let times = tracked(&mut failures, key, now);
    if times.len() >= self.limit {
      if let Some(suppressed) = TRIPPED.allow() {
        warn!(%ip, kind = self.what, suppressed, "rate limit tripped");
      }
      return None;
    }
    times.push_back(now);
    Some(Attempt { key, at: now })
  }

  /// Takes back the failure `attempt` counted, for an attempt that succeeded.
  pub fn forgive(&self, attempt: Attempt) {
    let mut failures = lock(&self.failures);
    let Some(times) = failures.get_mut(&attempt.key) else {
      // the address was forgotten to make room, so there is nothing to take back
      return;
    };
    if let Some(position) = times.iter().position(|at| *at == attempt.at) {
      times.remove(position);
    }
    if times.is_empty() {
      failures.remove(&attempt.key);
    }
  }
}

/// An attempt [`Window::attempt`] counted.
pub struct Attempt {
  key: IpAddr,
  at: Instant,
}

/// The failures of `key` with the old ones forgotten, making room for it when [`TRACKED`]
/// addresses are tracked already.
fn tracked(
  failures: &mut HashMap<IpAddr, VecDeque<Instant>>,
  key: IpAddr,
  now: Instant,
) -> &mut VecDeque<Instant> {
  if failures.len() >= TRACKED && !failures.contains_key(&key) {
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
  let times = failures.entry(key).or_default();
  forget_old(times, now);
  times
}

/// The address failures of `ip` are counted under: an IPv4 address itself, an IPv6 address its
/// /64.
fn counted_as(ip: IpAddr) -> IpAddr {
  match ip {
    IpAddr::V4(_) => ip,
    IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
      Some(v4) => IpAddr::V4(v4),
      None => IpAddr::V6(Ipv6Addr::from_bits(v6.to_bits() & !u128::from(u64::MAX))),
    },
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

  #[tokio::test(start_paused = true)]
  async fn attempts_started_at_once_are_held_to_the_limit() {
    let limits = Limits::default();
    let ip: IpAddr = "192.0.2.1".parse().unwrap();
    let started: Vec<Attempt> = (0..50).filter_map(|_| limits.logins.attempt(ip)).collect();
    assert_eq!(
      started.len(),
      LOGINS,
      "the rest are refused before any finishes"
    );
    assert!(limits.logins.blocked(ip));
    let mut started = started.into_iter();
    limits.logins.forgive(started.next().unwrap());
    assert!(!limits.logins.blocked(ip), "a success gives its place back");
    assert!(limits.logins.attempt(ip).is_some());
    assert!(limits.logins.attempt(ip).is_none());
    tokio::time::advance(WINDOW).await;
    assert!(
      limits.logins.attempt(ip).is_some(),
      "the failures left the window"
    );
  }

  #[tokio::test(start_paused = true)]
  async fn an_ipv6_client_is_counted_by_its_64() {
    let limits = Limits::default();
    let address = |text: &str| text.parse::<IpAddr>().unwrap();
    for host in 1..=5 {
      limits
        .logins
        .failed(address(&format!("2001:db8:1:2::{host:x}")));
    }
    assert!(limits.logins.blocked(address("2001:db8:1:2:ffff::1")));
    assert!(
      !limits.logins.blocked(address("2001:db8:1:3::1")),
      "another /64"
    );
    for _ in 0..5 {
      limits.logins.failed(address("::ffff:192.0.2.9"));
    }
    assert!(
      limits.logins.blocked(address("192.0.2.9")),
      "a mapped address is its IPv4 one"
    );
    assert!(!limits.logins.blocked(address("192.0.2.10")));
  }
}
