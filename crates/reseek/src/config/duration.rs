//! How a duration option is read and written: read as any humantime string, written in the
//! largest unit that holds it exactly, so 180 days reads back as `180d` and 6 months as
//! `6months`, each as it was typed.

use std::time::Duration;

use serde::{Deserialize, Deserializer, Serializer};

/// Humantime's year and month, 365.25 and 30.44 days, in nanoseconds, with how one and many of
/// each are written. A value is written in them only when it is a whole number of them, which a
/// count of days almost never is, so `180d` stays `180d`.
const CALENDAR: [(u128, &str, &str); 2] = [
  (31_557_600_000_000_000, "year", "years"),
  (2_630_016_000_000_000, "month", "months"),
];

/// The other units an option is written in, largest first, in nanoseconds. Weeks are left out:
/// `14d` reads as what was set where `2w` does not.
const UNITS: [(&str, u128); 6] = [
  ("d", 86_400_000_000_000),
  ("h", 3_600_000_000_000),
  ("m", 60_000_000_000),
  ("s", 1_000_000_000),
  ("ms", 1_000_000),
  ("us", 1_000),
];

/// `duration` in the largest unit that holds it exactly: `6months`, `180d`, `90m`, `1500ms`,
/// `0s`.
pub fn written(duration: Duration) -> String {
  let nanos = duration.as_nanos();
  if nanos == 0 {
    return "0s".to_string();
  }
  for (size, one, many) in CALENDAR {
    if nanos.is_multiple_of(size) {
      let count = nanos / size;
      return format!("{count}{}", if count == 1 { one } else { many });
    }
  }
  for (unit, size) in UNITS {
    if nanos.is_multiple_of(size) {
      return format!("{}{unit}", nanos / size);
    }
  }
  format!("{nanos}ns")
}

/// A duration option's value: a duration, or one that may be left unset.
pub trait Written {
  /// The value as written, `None` when unset.
  fn text(&self) -> Option<String>;
}

impl Written for Duration {
  fn text(&self) -> Option<String> {
    Some(written(*self))
  }
}

impl Written for Option<Duration> {
  fn text(&self) -> Option<String> {
    self.map(written)
  }
}

/// Writes a duration option, for `#[serde(with = "crate::config::duration")]`.
pub fn serialize<T: Written, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
  match value.text() {
    Some(text) => serializer.serialize_some(&text),
    None => serializer.serialize_none(),
  }
}

/// Reads a duration option from any humantime string.
pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
  humantime_serde::Serde<T>: Deserialize<'de>,
  D: Deserializer<'de>,
{
  humantime_serde::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::written;
  use crate::config::Retention;

  #[test]
  fn an_option_reads_back_in_the_unit_it_was_set_in() {
    for typed in ["180d", "6months", "1year"] {
      let set = serde_json::json!({ "search": typed });
      let retention: Retention = serde_json::from_value(set).unwrap();
      let written = serde_json::to_value(&retention).unwrap();
      assert_eq!(written["search"], typed);
      assert_eq!(written["users"], serde_json::Value::Null);
      let read: Retention = serde_json::from_value(written).unwrap();
      assert_eq!(read, retention);
    }
  }

  #[test]
  fn a_duration_is_written_in_the_largest_unit_that_holds_it() {
    let cases = [
      (Duration::ZERO, "0s"),
      (Duration::from_secs(6 * 3600), "6h"),
      (Duration::from_secs(14 * 24 * 3600), "14d"),
      (Duration::from_secs(90 * 60), "90m"),
      (Duration::from_secs(25 * 3600), "25h"),
      (Duration::from_millis(1500), "1500ms"),
      (Duration::from_nanos(1_000_001), "1000001ns"),
      (Duration::from_secs(2_630_016), "1month"),
      (Duration::from_secs(2 * 31_557_600), "2years"),
    ];
    for (duration, text) in cases {
      assert_eq!(written(duration), text);
      assert_eq!(humantime::parse_duration(text).unwrap(), duration);
    }
  }
}
