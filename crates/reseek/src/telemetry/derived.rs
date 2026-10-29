//! Gauges fed by a helper rather than set directly: a count over the last whole minute, and a
//! moving average.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use super::registry::Gauge;
use crate::sync::lock;

/// Counts over a window; when the window ends the count is published to its gauge and starts
/// again from zero, so the gauge shows the count of the last whole window. The window is a
/// minute, ended by the metrics' clock.
#[derive(Debug)]
pub struct TimedCounter {
  count: AtomicU64,
  gauge: Gauge,
}

impl TimedCounter {
  /// A counter publishing to `gauge`.
  pub fn new(gauge: Gauge) -> Self {
    Self {
      count: AtomicU64::new(0),
      gauge,
    }
  }

  /// Adds `n` to the window's count.
  pub fn count_up(&self, n: u64) {
    self.count.fetch_add(n, Ordering::Relaxed);
  }

  /// The count of the window so far, not yet published.
  pub fn current(&self) -> u64 {
    self.count.load(Ordering::Relaxed)
  }

  /// Ends the window: publishes its count and starts the next at zero.
  pub fn elapse(&self) {
    let count = self.count.swap(0, Ordering::Relaxed);
    self.gauge.set(count as f64);
  }
}

/// An exponential moving average with a weight of one half for each new value, published to
/// its gauge on every update.
#[derive(Debug)]
pub struct Ema {
  value: Mutex<Option<f64>>,
  gauge: Gauge,
}

/// How much a new value weighs against the average.
const ALPHA: f64 = 0.5;

impl Ema {
  /// An average publishing to `gauge`, empty until its first value.
  pub fn new(gauge: Gauge) -> Self {
    Self {
      value: Mutex::new(None),
      gauge,
    }
  }

  /// Moves the average toward `x` and returns it; the first value is the average.
  pub fn update(&self, x: f64) -> f64 {
    let mut value = lock(&self.value);
    let next = value.map_or(x, |v| v + (x - v) * ALPHA);
    *value = Some(next);
    self.gauge.set(next);
    next
  }

  /// The average as last published, 0 while empty.
  pub fn current(&self) -> f64 {
    self.gauge.get()
  }

  /// Empties the average and publishes 0.
  pub fn reset(&self) {
    *lock(&self.value) = None;
    self.gauge.set(0.0);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_timed_counter_publishes_the_last_whole_window() {
    let gauge = Gauge::default();
    let counter = TimedCounter::new(gauge.clone());
    counter.count_up(3);
    counter.count_up(2);
    assert_eq!((counter.current(), gauge.get()), (5, 0.0));
    counter.elapse();
    assert_eq!((counter.current(), gauge.get()), (0, 5.0));
    counter.elapse();
    assert_eq!(gauge.get(), 0.0, "an empty window publishes zero");
  }

  #[test]
  fn the_average_halves_the_way_to_each_value_and_resets_to_empty() {
    let gauge = Gauge::default();
    let ema = Ema::new(gauge.clone());
    ema.update(10.0);
    assert_eq!(gauge.get(), 10.0);
    ema.update(20.0);
    assert_eq!(gauge.get(), 15.0);
    ema.reset();
    assert_eq!(gauge.get(), 0.0);
    ema.update(4.0);
    assert_eq!(
      gauge.get(),
      4.0,
      "after a reset the first value is the average"
    );
  }
}
