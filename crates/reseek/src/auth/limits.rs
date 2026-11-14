//! Limits on failed attempts per client address: failed logins and failed API credentials, each
//! counted over a sliding minute and kept in memory only. An IPv6 client is counted by its /64,
//! the block one host is usually given, so it cannot spread attempts over its own addresses. An
//! attempt holds a place from when it starts until it ends, and keeps it as a failure unless it
//! is forgiven. An address refused for its failures is answered at once; one whose attempts under
//! way take the places its failures leave waits for one of them to end, so attempts made at the
//! same time are held to the limit without refusing those that succeed.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;
use std::time::Duration;

use reseek_shares::lock;
use soulseek_client::LogLimiter;
use tokio::sync::Notify;
use tokio::time::Instant;
use tracing::warn;

/// How long a failure counts.
pub const WINDOW: Duration = Duration::from_secs(60);

/// Failed logins an address may make in [`WINDOW`].
const LOGINS: usize = 5;

/// Failed token or API key presentations an address may make in [`WINDOW`].
const PRESENTATIONS: usize = 20;

/// The most addresses tracked at once; past it the oldest without an attempt under way are
/// forgotten first.
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
  addresses: Mutex<HashMap<IpAddr, Address>>,
  /// told whenever an attempt ends, for the attempts waiting for a place
  ended: Notify,
}

/// What one address has used of the limit.
#[derive(Default)]
struct Address {
  /// when each failure still in the window happened
  failures: VecDeque<Instant>,
  /// attempts started and not ended
  running: usize,
}

impl Window {
  fn new(what: &'static str, limit: usize) -> Self {
    Self {
      what,
      limit,
      addresses: Mutex::new(HashMap::new()),
      ended: Notify::new(),
    }
  }

  /// Starts an attempt of `ip`, waiting while its attempts under way take the places its
  /// failures leave; `None` when `ip` has failed as often as it may within the window, which is
  /// logged.
  pub async fn attempt(&self, ip: IpAddr) -> Option<Attempt<'_>> {
    let key = counted_as(ip);
    loop {
      // made before the counts are read, so an attempt that ends in between still wakes it
      let ended = self.ended.notified();
      {
        let mut addresses = lock(&self.addresses);
        let address = tracked(&mut addresses, key, Instant::now());
        if address.failures.len() >= self.limit {
          if let Some(suppressed) = TRIPPED.allow() {
            warn!(%ip, kind = self.what, suppressed, "rate limit tripped");
          }
          return None;
        }
        if address.failures.len() + address.running < self.limit {
          address.running += 1;
          return Some(Attempt {
            window: self,
            key,
            failed: true,
          });
        }
      }
      ended.await;
    }
  }

  /// Ends an attempt of the address `key`, keeping it as a failure when `failed`.
  fn end(&self, key: IpAddr, failed: bool) {
    let mut addresses = lock(&self.addresses);
    // an address with an attempt under way is never forgotten
    let address = addresses
      .get_mut(&key)
      .expect("an address is tracked while its attempts run");
    address.running -= 1;
    if failed {
      address.failures.push_back(Instant::now());
    }
    if address.failures.is_empty() && address.running == 0 {
      addresses.remove(&key);
    }
    drop(addresses);
    self.ended.notify_waiters();
  }
}

/// An attempt [`Window::attempt`] started. It ends when dropped, as a failure unless it was
/// forgiven, so an attempt whose request went away before an answer still counts.
pub struct Attempt<'a> {
  window: &'a Window,
  key: IpAddr,
  failed: bool,
}

impl Attempt<'_> {
  /// Ends the attempt without counting it, for one that succeeded or was never checked.
  pub fn forgive(mut self) {
    self.failed = false;
  }
}

impl Drop for Attempt<'_> {
  fn drop(&mut self) {
    self.window.end(self.key, self.failed);
  }
}

/// The address `key` with its old failures forgotten, making room for it when [`TRACKED`]
/// addresses are tracked already.
fn tracked(addresses: &mut HashMap<IpAddr, Address>, key: IpAddr, now: Instant) -> &mut Address {
  if addresses.len() >= TRACKED && !addresses.contains_key(&key) {
    addresses.retain(|_, address| {
      forget_old(&mut address.failures, now);
      !address.failures.is_empty() || address.running > 0
    });
    if addresses.len() >= TRACKED {
      let oldest = addresses
        .iter()
        .filter(|(_, address)| address.running == 0)
        .min_by_key(|(_, address)| address.failures.back().copied())
        .map(|(ip, _)| *ip);
      if let Some(oldest) = oldest {
        addresses.remove(&oldest);
      }
    }
  }
  let address = addresses.entry(key).or_default();
  forget_old(&mut address.failures, now);
  address
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
  use futures_util::FutureExt;

  use super::*;

  #[tokio::test(start_paused = true)]
  async fn an_address_is_blocked_for_a_sliding_minute_after_its_failures() {
    let limits = Limits::default();
    let ip: IpAddr = "192.0.2.1".parse().unwrap();
    let other: IpAddr = "192.0.2.2".parse().unwrap();
    // each attempt is dropped as soon as it starts, which counts it as a failure
    for _ in 0..4 {
      assert!(limits.logins.attempt(ip).await.is_some());
    }
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(limits.logins.attempt(ip).await.is_some());
    assert!(limits.logins.attempt(ip).await.is_none());
    assert!(limits.logins.attempt(other).await.is_some());
    assert!(
      limits.presentations.attempt(ip).await.is_some(),
      "the limits are apart"
    );
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(
      limits.logins.attempt(ip).await.is_some(),
      "the first four left the window"
    );
    for _ in 0..PRESENTATIONS {
      assert!(limits.presentations.attempt(other).await.is_some());
    }
    assert!(limits.presentations.attempt(other).await.is_none());
  }

  #[tokio::test(start_paused = true)]
  async fn attempts_under_way_hold_their_places_and_the_next_waits_for_one() {
    let limits = Limits::default();
    let ip: IpAddr = "192.0.2.1".parse().unwrap();
    let mut running = Vec::new();
    for _ in 0..LOGINS {
      running.push(limits.logins.attempt(ip).await.unwrap());
    }
    let next = limits.logins.attempt(ip);
    tokio::pin!(next);
    assert!(
      next.as_mut().now_or_never().is_none(),
      "it waits for a place"
    );
    running.pop().unwrap().forgive();
    let next = next.await.expect("a success gives its place to the next");
    running.push(next);
    let after = limits.logins.attempt(ip);
    tokio::pin!(after);
    assert!(after.as_mut().now_or_never().is_none());
    // the five under way fail, which refuses the one waiting
    drop(running);
    assert!(after.await.is_none());
    tokio::time::advance(WINDOW).await;
    assert!(
      limits.logins.attempt(ip).await.is_some(),
      "the failures left the window"
    );
  }

  #[tokio::test(start_paused = true)]
  async fn an_ipv6_client_is_counted_by_its_64() {
    let limits = Limits::default();
    let address = |text: &str| text.parse::<IpAddr>().unwrap();
    for host in 1..=LOGINS {
      let host = address(&format!("2001:db8:1:2::{host:x}"));
      assert!(limits.logins.attempt(host).await.is_some());
    }
    assert!(
      limits
        .logins
        .attempt(address("2001:db8:1:2:ffff::1"))
        .await
        .is_none()
    );
    assert!(
      limits
        .logins
        .attempt(address("2001:db8:1:3::1"))
        .await
        .is_some(),
      "another /64"
    );
    for _ in 0..LOGINS {
      assert!(
        limits
          .logins
          .attempt(address("::ffff:192.0.2.9"))
          .await
          .is_some()
      );
    }
    assert!(
      limits.logins.attempt(address("192.0.2.9")).await.is_none(),
      "a mapped address is its IPv4 one"
    );
    assert!(limits.logins.attempt(address("192.0.2.10")).await.is_some());
  }
}
