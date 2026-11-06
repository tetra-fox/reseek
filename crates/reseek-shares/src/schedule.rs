//! How often a piece of recurring work runs, as an option writes it: `off`, an interval such as
//! `5m` or `12h`, or a whole number of days at a time of day, such as `7d at 04:00`, and when
//! it is next due.

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Days, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A day, the unit an interval with a time of day is counted in.
const DAY: Duration = Duration::from_secs(24 * 3600);

/// How often a piece of recurring work runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Schedule {
  /// it does not run on its own
  #[default]
  Off,
  /// it runs `every` apart, at the time of day `at` when one is set; with `at`, `every` is a
  /// whole number of days
  Every {
    /// the time between two runs
    every: Duration,
    /// the local time of day it runs at
    at: Option<NaiveTime>,
  },
}

impl Schedule {
  /// Runs `every` apart.
  pub const fn every(every: Duration) -> Self {
    Self::Every { every, at: None }
  }

  /// The time between two runs, none when it is off.
  pub fn interval(&self) -> Option<Duration> {
    match self {
      Self::Off => None,
      Self::Every { every, .. } => Some(*every),
    }
  }

  /// When the work is next due, seen at `now` in the time zone `zone`, after its last run at
  /// `last`; `None` when it is off. A work never run is due at once, or with a time of day at
  /// the next time it shows. A run missed while nothing ran is due at once, or with a time of
  /// day at the next time it shows, so it does not wait a whole interval more.
  pub fn next<Z: TimeZone>(
    &self,
    last: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    zone: &Z,
  ) -> Option<DateTime<Utc>> {
    let Self::Every { every, at } = *self else {
      return None;
    };
    let Some(at) = at else {
      let due = last.map_or(now, |last| {
        chrono::Duration::from_std(every).map_or(DateTime::<Utc>::MAX_UTC, |every| {
          last
            .checked_add_signed(every)
            .unwrap_or(DateTime::<Utc>::MAX_UTC)
        })
      });
      return Some(due.max(now));
    };
    let days = (every.as_secs() / DAY.as_secs()).max(1);
    let first_after_now = next_at(now, at, zone);
    let Some(last) = last else {
      return Some(first_after_now);
    };
    let date = last
      .with_timezone(zone)
      .date_naive()
      .checked_add_days(Days::new(days))?;
    let due = local(date.and_time(at), zone);
    Some(if due < now { first_after_now } else { due })
  }

  /// Checks the schedule can be used, with `least` the shortest interval it may have: a time
  /// of day needs a whole number of days.
  pub fn check(&self, least: Duration) -> Result<(), String> {
    let Self::Every { every, at } = self else {
      return Ok(());
    };
    if *every < least {
      return Err(format!(
        "The interval must be at least {}",
        cascade::duration::written(least)
      ));
    }
    if at.is_some() && !every.as_secs().is_multiple_of(DAY.as_secs()) {
      return Err("A time of day needs an interval of whole days, such as 1d or 7d".to_string());
    }
    Ok(())
  }
}

/// The first time after `now` the local clock of `zone` shows `at`.
fn next_at<Z: TimeZone>(now: DateTime<Utc>, at: NaiveTime, zone: &Z) -> DateTime<Utc> {
  let today = now.with_timezone(zone).date_naive();
  let due = local(today.and_time(at), zone);
  if due > now {
    return due;
  }
  today
    .checked_add_days(Days::new(1))
    .map_or(DateTime::<Utc>::MAX_UTC, |tomorrow| {
      local(tomorrow.and_time(at), zone)
    })
}

/// The moment the local clock of `zone` shows `time`; the earlier one when the clock shows it
/// twice, and an hour later when a change of the clock skips it.
fn local<Z: TimeZone>(time: chrono::NaiveDateTime, zone: &Z) -> DateTime<Utc> {
  let found = zone.from_local_datetime(&time).earliest().or_else(|| {
    zone
      .from_local_datetime(&(time + chrono::Duration::hours(1)))
      .earliest()
  });
  found.map_or(DateTime::<Utc>::MAX_UTC, |found| found.with_timezone(&Utc))
}

impl fmt::Display for Schedule {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Off => f.write_str("off"),
      Self::Every { every, at: None } => f.write_str(&cascade::duration::written(*every)),
      Self::Every {
        every,
        at: Some(at),
      } => write!(
        f,
        "{} at {}",
        cascade::duration::written(*every),
        at.format("%H:%M")
      ),
    }
  }
}

impl std::str::FromStr for Schedule {
  type Err = String;

  fn from_str(text: &str) -> Result<Self, String> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("off") {
      return Ok(Self::Off);
    }
    let (interval, at) = match text.split_once(" at ") {
      Some((interval, at)) => (interval.trim(), Some(at.trim())),
      None => (text, None),
    };
    let every = humantime::parse_duration(interval)
      .map_err(|e| format!("`{interval}` is not an interval such as 5m or 7d: {e}"))?;
    if every.is_zero() {
      return Err("The interval must be longer than zero; write `off` to turn it off".to_string());
    }
    let at = at
      .map(|at| {
        NaiveTime::parse_from_str(at, "%H:%M")
          .map_err(|_| format!("`{at}` is not a time of day such as 04:00"))
      })
      .transpose()?;
    Ok(Self::Every { every, at })
  }
}

impl Serialize for Schedule {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&self.to_string())
  }
}

impl<'de> Deserialize<'de> for Schedule {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    // a schedule left unset in an older file reads as off
    let text = Option::<String>::deserialize(deserializer)?;
    match text {
      None => Ok(Self::Off),
      Some(text) => text.parse().map_err(serde::de::Error::custom),
    }
  }
}

impl cascade::Cascade for Schedule {
  fn shape() -> cascade::Shape {
    cascade::Shape::leaf(cascade::Kind::String)
  }
}

#[cfg(test)]
mod tests {
  use chrono::{FixedOffset, LocalResult, NaiveDate, NaiveDateTime};

  use super::*;

  /// A zone an hour ahead of UTC in winter and two in summer, its clocks going forward at
  /// 2026-03-29 01:00 UTC (02:00 is skipped) and back at 2026-10-25 01:00 UTC (02:00 to 03:00
  /// shows twice), as central Europe's do.
  #[derive(Clone, Copy, Debug)]
  struct Central;

  impl Central {
    fn winter() -> FixedOffset {
      FixedOffset::east_opt(3600).unwrap()
    }

    fn summer() -> FixedOffset {
      FixedOffset::east_opt(7200).unwrap()
    }
  }

  impl TimeZone for Central {
    type Offset = FixedOffset;

    fn from_offset(_: &FixedOffset) -> Self {
      Central
    }

    fn offset_from_local_date(&self, local: &NaiveDate) -> LocalResult<FixedOffset> {
      self.offset_from_local_datetime(&local.and_hms_opt(12, 0, 0).unwrap())
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<FixedOffset> {
      let as_winter = *local - chrono::Duration::hours(1);
      let as_summer = *local - chrono::Duration::hours(2);
      let winter = self.offset_from_utc_datetime(&as_winter) == Central::winter();
      let summer = self.offset_from_utc_datetime(&as_summer) == Central::summer();
      match (summer, winter) {
        (true, true) => LocalResult::Ambiguous(Central::summer(), Central::winter()),
        (true, false) => LocalResult::Single(Central::summer()),
        (false, true) => LocalResult::Single(Central::winter()),
        (false, false) => LocalResult::None,
      }
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
      self.offset_from_utc_datetime(&utc.and_hms_opt(12, 0, 0).unwrap())
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
      let forward = at("2026-03-29T01:00:00Z").naive_utc();
      let back = at("2026-10-25T01:00:00Z").naive_utc();
      if (forward..back).contains(utc) {
        Central::summer()
      } else {
        Central::winter()
      }
    }
  }

  #[test]
  fn a_time_of_day_the_clock_skips_runs_an_hour_later_and_one_it_shows_twice_runs_first() {
    let nightly = schedule("1d at 02:30");
    let now = at("2026-03-28T12:00:00Z");
    assert_eq!(
      nightly.next(Some(at("2026-03-28T01:30:00Z")), now, &Central),
      Some(at("2026-03-29T01:30:00Z")),
      "02:30 does not exist on the 29th; 03:30 summer time is 01:30 UTC"
    );
    let now = at("2026-10-24T12:00:00Z");
    assert_eq!(
      nightly.next(Some(at("2026-10-24T00:30:00Z")), now, &Central),
      Some(at("2026-10-25T00:30:00Z")),
      "02:30 shows twice on the 25th; the first is still summer time"
    );
    assert_eq!(
      nightly.next(Some(at("2026-10-25T00:30:00Z")), now, &Central),
      Some(at("2026-10-26T01:30:00Z")),
      "back in winter time the next day"
    );
  }

  fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().to_utc()
  }

  fn schedule(text: &str) -> Schedule {
    text.parse().unwrap()
  }

  #[test]
  fn schedules_read_and_write_as_typed() {
    for typed in ["off", "5m", "12h", "7d at 04:00", "1d at 23:30"] {
      assert_eq!(schedule(typed).to_string(), typed);
    }
    assert_eq!(schedule(" OFF "), Schedule::Off);
    assert_eq!(schedule("1week").to_string(), "7d");
    assert!("0s".parse::<Schedule>().is_err());
    assert!("soon".parse::<Schedule>().is_err());
    assert!("1d at 25:00".parse::<Schedule>().is_err());
    assert!("1d at noon".parse::<Schedule>().is_err());
    let hour = Duration::from_secs(3600);
    assert!(schedule("12h at 04:00").check(hour).is_err());
    assert!(schedule("30m").check(hour).is_err());
    assert!(schedule("2d at 04:00").check(hour).is_ok());
    assert!(Schedule::Off.check(hour).is_ok());
  }

  #[test]
  fn an_interval_is_due_that_long_after_the_last_run_or_at_once() {
    let now = at("2026-11-05T12:00:00Z");
    let every = schedule("5m");
    assert_eq!(every.next(None, now, &Utc), Some(now), "never run");
    assert_eq!(
      every.next(Some(at("2026-11-05T11:58:00Z")), now, &Utc),
      Some(at("2026-11-05T12:03:00Z"))
    );
    assert_eq!(
      every.next(Some(at("2026-11-04T00:00:00Z")), now, &Utc),
      Some(now),
      "a missed run is due at once"
    );
    assert_eq!(Schedule::Off.next(None, now, &Utc), None);
  }

  #[test]
  fn a_time_of_day_is_kept_in_the_local_zone() {
    // two hours ahead of UTC, so 04:00 there is 02:00 UTC
    let zone = FixedOffset::east_opt(2 * 3600).unwrap();
    let weekly = schedule("7d at 04:00");
    let now = at("2026-11-05T12:00:00Z");
    assert_eq!(
      weekly.next(None, now, &zone),
      Some(at("2026-11-06T02:00:00Z")),
      "never run: the next 04:00"
    );
    assert_eq!(
      weekly.next(None, at("2026-11-05T01:00:00Z"), &zone),
      Some(at("2026-11-05T02:00:00Z")),
      "the next 04:00 can be today"
    );
    assert_eq!(
      weekly.next(Some(at("2026-11-03T02:00:00Z")), now, &zone),
      Some(at("2026-11-10T02:00:00Z")),
      "a week after the last run's day"
    );
    assert_eq!(
      weekly.next(Some(at("2026-11-03T15:30:00Z")), now, &zone),
      Some(at("2026-11-10T02:00:00Z")),
      "a run late in the day keeps the time of day"
    );
    assert_eq!(
      weekly.next(Some(at("2026-10-01T02:00:00Z")), now, &zone),
      Some(at("2026-11-06T02:00:00Z")),
      "a missed run waits for the next 04:00, not a whole week"
    );
    // just before midnight UTC is already the next day two hours ahead
    assert_eq!(
      schedule("1d at 04:00").next(Some(at("2026-11-04T23:00:00Z")), now, &zone),
      Some(at("2026-11-06T02:00:00Z"))
    );
  }
}
