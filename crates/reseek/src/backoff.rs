//! How long a loop that keeps failing waits before it tries again: the first retry follows at
//! once, then after about 1, 3, 7, 15 seconds and so on up to a cap, each with up to a second
//! of jitter so many loops failing together do not retry together.

use std::time::Duration;

/// The wait before the retry after `failures` failed attempts: at most `cap` plus the jitter.
pub fn wait(failures: u32, cap: Duration) -> Duration {
  without_jitter(failures, cap) + Duration::from_millis(rand::random_range(0..1000))
}

/// Half of `2^failures - 1` seconds, rounded down, at most `cap`.
fn without_jitter(failures: u32, cap: Duration) -> Duration {
  let seconds = ((1u128 << failures.min(100)) - 1) / 2;
  // at most `cap`, whose seconds fit a u64
  Duration::from_secs(u64::try_from(seconds).unwrap_or(u64::MAX)).min(cap)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_wait_doubles_from_nothing_up_to_its_cap() {
    let cap = Duration::from_secs(300);
    let delays: Vec<u64> = (0..=11)
      .map(|failures| without_jitter(failures, cap).as_secs())
      .collect();
    assert_eq!(delays, [0, 0, 1, 3, 7, 15, 31, 63, 127, 255, 300, 300]);
    assert_eq!(without_jitter(u32::MAX, cap), cap, "capped");
    let whole = |failures| wait(failures, Duration::from_secs(30)).as_secs();
    let jittered: Vec<u64> = (1..=7).map(whole).collect();
    assert_eq!(
      jittered,
      [0, 1, 3, 7, 15, 30, 30],
      "under a second of jitter"
    );
  }
}
