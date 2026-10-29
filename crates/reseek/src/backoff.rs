//! How long a loop that keeps failing waits before it tries again. Each loop's waits grow by one
//! rule: the first retry waits a `first` delay, and each later one twice the one before plus a
//! `step`, up to a cap, each with up to a second of jitter so many loops failing together do not
//! retry together.

use std::time::Duration;

/// How a loop's waits grow: the first retry waits `first`, each later one twice the one before
/// plus `step`.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
  first: Duration,
  step: Duration,
}

/// The first retry follows at once, then after about 1, 3, 7, 15 seconds and so on: for a
/// connection that should come back as soon as it can.
pub const AT_ONCE: Rule = Rule {
  first: Duration::ZERO,
  step: Duration::from_secs(1),
};

/// The first retry after `first`, then each wait twice the one before: for a peer that should be
/// given time.
pub const fn doubling(first: Duration) -> Rule {
  Rule {
    first,
    step: Duration::ZERO,
  }
}

/// The wait before the retry after `failures` failed attempts: at most `cap` plus the jitter.
pub fn wait(failures: u32, rule: Rule, cap: Duration) -> Duration {
  without_jitter(failures, rule, cap) + Duration::from_millis(rand::random_range(0..1000))
}

/// `2^(failures - 1)` times `first` plus `2^(failures - 1) - 1` times `step`, at most `cap`;
/// nothing before the first failure.
fn without_jitter(failures: u32, rule: Rule, cap: Duration) -> Duration {
  let Some(doublings) = failures.checked_sub(1) else {
    return Duration::ZERO;
  };
  let factor = 1u128 << doublings.min(100);
  let millis = factor
    .saturating_mul(rule.first.as_millis())
    .saturating_add((factor - 1).saturating_mul(rule.step.as_millis()));
  // at most `cap`, whose milliseconds fit a u64
  Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX)).min(cap)
}

#[cfg(test)]
mod tests {
  use super::*;

  const SECOND: Duration = Duration::from_secs(1);

  #[test]
  fn at_once_the_wait_doubles_from_nothing_up_to_its_cap() {
    let cap = Duration::from_secs(300);
    let delays: Vec<u64> = (0..=11)
      .map(|failures| without_jitter(failures, AT_ONCE, cap).as_secs())
      .collect();
    assert_eq!(delays, [0, 0, 1, 3, 7, 15, 31, 63, 127, 255, 300, 300]);
    assert_eq!(without_jitter(u32::MAX, AT_ONCE, cap), cap, "capped");
    let whole = |failures| wait(failures, AT_ONCE, 30 * SECOND).as_secs();
    let jittered: Vec<u64> = (1..=7).map(whole).collect();
    assert_eq!(
      jittered,
      [0, 1, 3, 7, 15, 30, 30],
      "under a second of jitter"
    );
  }

  #[test]
  fn doubling_waits_its_first_delay_then_twice_the_last() {
    let rule = doubling(5 * SECOND);
    let delays: Vec<u64> = (1..=7)
      .map(|failures| without_jitter(failures, rule, 60 * SECOND).as_secs())
      .collect();
    assert_eq!(delays, [5, 10, 20, 40, 60, 60, 60]);
    assert_eq!(without_jitter(u32::MAX, rule, 60 * SECOND), 60 * SECOND);
  }
}
