//! Keeps a log line that can repeat every second from flooding the log: the first occurrence is
//! written, then at most one per minute carrying how many were left out in between.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

const INTERVAL: Duration = Duration::from_secs(60);

/// One per call site, usually a `static`
pub(crate) struct LogLimiter {
  state: Mutex<Option<(Instant, u64)>>,
}

impl LogLimiter {
  pub(crate) const fn new() -> Self {
    Self {
      state: Mutex::new(None),
    }
  }

  /// `Some(suppressed)` when a line may be written now, with how many were left out since the
  /// last one
  pub(crate) fn allow(&self) -> Option<u64> {
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
}
