//! The reports against what they say they count, worked out here row by row over a history
//! written the way the engines write it: hundreds of transfers of five users both ways, with
//! every outcome, unfinished and removed ones among them, and times to the microsecond across
//! ten days, two of them ending right on the window's ends.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use api_types::reports::{
  DirectoriesQuery, ExceptionsQuery, HistogramQuery, LeaderboardQuery, LeaderboardSort,
  ParetoQuery, SummaryQuery, TransferOutcome, TransferSummary,
};
use api_types::transfers::TransferDirection::{self, Download, Upload};
use api_types::transfers::{SortOrder, TransferStates};
use chrono::{DateTime, Duration, Utc};

use super::{Reports, Width, Window, outcome};
use crate::transfers::fixtures::transfers;
use crate::transfers::{Record, direction_text, store};

const USERS: [&str; 5] = ["alice", "bob", "carol", "dave", "erin"];
const STATES: [u32; 12] = [48, 48, 48, 48, 80, 144, 272, 528, 1040, 16, 2050, 8];
const FAILED: [u32; 5] = [80, 144, 272, 528, 1040];
const EXCEPTIONS: [Option<&str>; 4] = [
  Some("File not shared."),
  Some("Remote file not found: C:\\x"),
  Some("Transfer timed out"),
  None,
];

/// A small generator of numbers that repeat from run to run.
struct Dice(u64);

impl Dice {
  fn below(&mut self, n: u64) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0 % n
  }

  fn pick<T: Copy>(&mut self, items: &[T]) -> T {
    items[self.below(items.len() as u64) as usize]
  }

  /// `at` moved on by up to `seconds` and a fraction of a second.
  fn after(&mut self, at: DateTime<Utc>, seconds: u64) -> DateTime<Utc> {
    at + Duration::seconds(self.below(seconds) as i64)
      + Duration::microseconds(self.below(1_000_000) as i64)
  }
}

fn base() -> DateTime<Utc> {
  "2026-03-01T00:00:00Z".parse().unwrap()
}

/// The window most reports are asked about, its ends in the middle of seconds.
fn window() -> Window {
  Window {
    start: base() + Duration::days(2) + Duration::microseconds(123_456),
    end: base() + Duration::days(8) + Duration::microseconds(654_321),
  }
}

fn history() -> Vec<Record> {
  let mut dice = Dice(0x5eed_cafe_f00d);
  let mut records = Vec::new();
  for n in 0..400 {
    let username = dice.pick(&USERS);
    let filename = match dice.below(3) {
      0 => format!(
        "@@music\\{}\\Album {}\\{n}.flac",
        dice.pick(&USERS),
        dice.below(3)
      ),
      1 => format!("music/{}/{n}.mp3", dice.pick(&USERS)),
      _ => format!("{n}.ogg"),
    };
    let size = 1 + dice.below(50_000_000);
    let mut record = if dice.below(2) == 0 {
      Record::download(username, &filename, size, None)
    } else {
      Record::upload(username, &filename, size)
    };
    let state = dice.pick(&STATES);
    record.state = TransferStates::from_bits_retain(state);
    record.requested_at = dice.after(base(), 10 * 86_400);
    record.enqueued_at = (dice.below(5) > 0).then(|| dice.after(record.requested_at, 600));
    record.started_at = match record.enqueued_at {
      Some(enqueued) if dice.below(4) > 0 => Some(dice.after(enqueued, 300)),
      _ => None,
    };
    if state & 16 != 0 {
      let from = record.started_at.unwrap_or(record.requested_at);
      record.ended_at = Some(dice.after(from, 3600));
    }
    record.average_speed = dice.below(10_000_000) as f64 / 7.0;
    record.bytes_transferred = dice.below(size);
    if FAILED.contains(&state) {
      record.exception = dice.pick(&EXCEPTIONS).map(str::to_string);
    }
    record.removed = dice.below(5) == 0;
    records.push(record);
  }
  for ended in [window().start, window().end] {
    let mut record = Record::upload("alice", "music/edge/x.mp3", 10);
    record.state = TransferStates::Completed | TransferStates::Succeeded;
    record.requested_at = ended - Duration::minutes(5);
    record.ended_at = Some(ended);
    records.push(record);
  }
  records
}

async fn written(records: &[Record]) -> Reports {
  let db = transfers().await;
  let records = records.to_vec();
  db.write(move |conn| {
    for record in &records {
      store::insert(conn, record)?;
    }
    Ok(())
  })
  .await
  .unwrap();
  Reports::new(db)
}

fn bits(record: &Record) -> u32 {
  record.state.bits()
}

fn ended_in(record: &Record, window: Window) -> bool {
  record
    .ended_at
    .is_some_and(|ended| window.start <= ended && ended <= window.end)
}

/// Whole seconds since the epoch, the fraction cut off.
fn unix(at: DateTime<Utc>) -> i64 {
  at.timestamp()
}

fn mean(values: &[f64]) -> f64 {
  if values.is_empty() {
    0.0
  } else {
    values.iter().sum::<f64>() / values.len() as f64
  }
}

/// The waits from queued to started, and the times from started to ended, of `rows`.
fn waits_and_durations(rows: &[&Record]) -> (Vec<f64>, Vec<f64>) {
  let waits = rows
    .iter()
    .filter_map(|r| Some((unix(r.started_at?) - unix(r.enqueued_at?)) as f64))
    .collect();
  let durations = rows
    .iter()
    .filter_map(|r| Some((unix(r.ended_at?) - unix(r.started_at?)) as f64))
    .collect();
  (waits, durations)
}

fn summarised(rows: &[&Record]) -> TransferSummary {
  let (waits, durations) = waits_and_durations(rows);
  let speeds: Vec<f64> = rows.iter().map(|r| r.average_speed).collect();
  TransferSummary {
    total_bytes: rows.iter().map(|r| r.size).sum(),
    count: rows.len() as u64,
    distinct_users: rows
      .iter()
      .map(|r| &r.username)
      .collect::<BTreeSet<_>>()
      .len() as u64,
    average_speed: mean(&speeds),
    average_wait: mean(&waits),
    average_duration: mean(&durations),
  }
}

/// The finished `rows` by outcome.
fn by_outcome(rows: &[&Record]) -> BTreeMap<TransferOutcome, TransferSummary> {
  let mut grouped: BTreeMap<TransferOutcome, Vec<&Record>> = BTreeMap::new();
  for row in rows {
    grouped.entry(outcome(bits(row))).or_default().push(row);
  }
  grouped
    .into_iter()
    .map(|(outcome, rows)| (outcome, summarised(&rows)))
    .collect()
}

fn same(found: &TransferSummary, expected: &TransferSummary, what: &str) {
  let close = |a: f64, b: f64| (a - b).abs() <= 1e-6 * b.abs().max(1.0);
  assert_eq!(
    (found.total_bytes, found.count, found.distinct_users),
    (
      expected.total_bytes,
      expected.count,
      expected.distinct_users
    ),
    "{what}"
  );
  assert!(
    close(found.average_speed, expected.average_speed),
    "{what}: {found:?} {expected:?}"
  );
  assert!(
    close(found.average_wait, expected.average_wait),
    "{what}: {found:?} {expected:?}"
  );
  assert!(
    close(found.average_duration, expected.average_duration),
    "{what}: {found:?} {expected:?}"
  );
}

fn same_outcomes(
  found: &BTreeMap<TransferOutcome, TransferSummary>,
  expected: &BTreeMap<TransferOutcome, TransferSummary>,
  what: &str,
) {
  assert_eq!(
    found.keys().collect::<Vec<_>>(),
    expected.keys().collect::<Vec<_>>(),
    "{what}"
  );
  for (outcome, summary) in expected {
    same(&found[outcome], summary, &format!("{what} {outcome:?}"));
  }
}

fn going(records: &[Record], direction: TransferDirection) -> impl Iterator<Item = &Record> {
  records.iter().filter(move |r| r.direction == direction)
}

/// A filename without its last segment and the separators before it.
fn directory(filename: &str) -> String {
  match filename.rfind(['/', '\\']) {
    Some(cut) => filename[..cut].trim_end_matches(['/', '\\']).to_string(),
    None => String::new(),
  }
}

#[tokio::test]
async fn the_totals_count_every_finished_transfer_in_the_window_by_outcome() {
  let records = history();
  let reports = written(&records).await;
  let window = window();
  for (direction, username) in [(None, None), (Some(Upload), None), (None, Some("bob"))] {
    let query = SummaryQuery {
      direction,
      username: username.map(str::to_string),
      ..SummaryQuery::default()
    };
    let found = reports.summary(window, query).await.unwrap();
    let picked: Vec<&Record> = records
      .iter()
      .filter(|r| ended_in(r, window) && bits(r) & 16 != 0)
      .filter(|r| direction.is_none_or(|d| r.direction == d))
      .filter(|r| username.is_none_or(|u| r.username == u))
      .collect();
    for (side, found) in [
      (Download, &found.directions.download),
      (Upload, &found.directions.upload),
    ] {
      let rows: Vec<&Record> = picked
        .iter()
        .copied()
        .filter(|r| r.direction == side)
        .collect();
      same_outcomes(
        found,
        &by_outcome(&rows),
        &format!("{direction:?} {username:?} {side:?}"),
      );
    }
    let succeeded: BTreeSet<&str> = picked
      .iter()
      .filter(|r| bits(r) == 48)
      .map(|r| r.username.as_str())
      .collect();
    assert_eq!(found.distinct_users, succeeded.len() as u64);
  }
}

#[tokio::test]
async fn the_histogram_buckets_by_epoch_aligned_ends_without_its_window_s_end() {
  let records = history();
  let reports = written(&records).await;
  let window = window();
  for width in [
    Width::Buckets(NonZeroU32::new(84).unwrap()),
    Width::Minutes(NonZeroU32::new(720).unwrap()),
  ] {
    let found = reports
      .histogram(window, width, HistogramQuery::default())
      .await
      .unwrap();
    let keys: Vec<i64> = found
      .keys()
      .map(|key| key.parse::<DateTime<Utc>>().unwrap().timestamp())
      .collect();
    let step = keys[1] - keys[0];
    assert!(
      keys.windows(2).all(|pair| pair[1] - pair[0] == step),
      "{width:?}"
    );
    assert_eq!(keys[0] % step, 0, "aligned to the epoch");
    assert!(keys[0] <= unix(window.start) && unix(window.start) < keys[0] + step);
    let last = *keys.last().unwrap();
    assert!(last < window.end.timestamp() + 1 && window.end.timestamp() < last + step);
    let mut expected: BTreeMap<(i64, &str), Vec<&Record>> = BTreeMap::new();
    let mut counted = 0;
    for record in &records {
      let Some(ended) = record.ended_at else {
        continue;
      };
      if bits(record) & 16 == 0 || ended < window.start || ended >= window.end {
        continue;
      }
      let key = unix(ended).div_euclid(step) * step;
      expected
        .entry((key, direction_text(record.direction)))
        .or_default()
        .push(record);
      counted += 1;
    }
    assert!(
      counted > 100 && expected.len() > 20,
      "{counted} in {} buckets",
      expected.len()
    );
    for (key, summaries) in &found {
      let at = key.parse::<DateTime<Utc>>().unwrap().timestamp();
      for (side, found) in [(Download, &summaries.download), (Upload, &summaries.upload)] {
        let rows = expected
          .remove(&(at, direction_text(side)))
          .unwrap_or_default();
        let mut wanted = by_outcome(&rows);
        for (outcome, summary) in wanted.iter_mut() {
          // a bucket's speed is its bytes over the seconds its transfers took
          let these: Vec<&Record> = rows
            .iter()
            .copied()
            .filter(|r| super::outcome(bits(r)) == *outcome)
            .collect();
          let seconds: f64 = waits_and_durations(&these).1.iter().sum();
          summary.average_speed = if seconds == 0.0 {
            0.0
          } else {
            summary.total_bytes as f64 / seconds
          };
        }
        same_outcomes(found, &wanted, &format!("{width:?} {key} {side:?}"));
      }
    }
    assert!(
      expected.is_empty(),
      "every transfer of the window is in a bucket"
    );
  }
}

#[tokio::test]
async fn the_rankings_order_users_and_directories_as_they_say() {
  let records = history();
  let reports = written(&records).await;
  let window = window();
  for direction in [Download, Upload] {
    let mut by_user: BTreeMap<&str, Vec<&Record>> = BTreeMap::new();
    for record in going(&records, direction).filter(|r| bits(r) == 48 && ended_in(r, window)) {
      by_user.entry(&record.username).or_default().push(record);
    }
    for sort_by in [
      LeaderboardSort::Count,
      LeaderboardSort::TotalBytes,
      LeaderboardSort::AverageSpeed,
    ] {
      for sort_order in [SortOrder::Asc, SortOrder::Desc] {
        let query = LeaderboardQuery {
          direction,
          start: None,
          end: None,
          sort_by,
          sort_order,
          limit: 3,
          offset: 1,
        };
        let found = reports.leaderboard(window, query).await.unwrap();
        let mut expected: Vec<(&str, TransferSummary)> = by_user
          .iter()
          .map(|(user, rows)| {
            (
              *user,
              TransferSummary {
                distinct_users: 1,
                ..summarised(rows)
              },
            )
          })
          .collect();
        let key = |s: &TransferSummary| match sort_by {
          LeaderboardSort::Count => s.count as f64,
          LeaderboardSort::TotalBytes => s.total_bytes as f64,
          LeaderboardSort::AverageSpeed => s.average_speed,
        };
        expected.sort_by(|a, b| {
          let ordered = key(&a.1).total_cmp(&key(&b.1));
          let ordered = if sort_order == SortOrder::Desc {
            ordered.reverse()
          } else {
            ordered
          };
          ordered.then(b.0.cmp(a.0))
        });
        let expected: Vec<_> = expected.into_iter().skip(1).take(3).collect();
        let names: Vec<&str> = found.iter().map(|e| e.username.as_str()).collect();
        let wanted: Vec<&str> = expected.iter().map(|(user, _)| *user).collect();
        assert_eq!(names, wanted, "{direction:?} {sort_by:?} {sort_order:?}");
        assert_eq!(names.len(), 3, "a full page");
        for (entry, (_, summary)) in found.iter().zip(&expected) {
          same(&entry.summary, summary, &entry.username);
        }
      }
    }
  }
  let found = reports
    .directories(
      window,
      DirectoriesQuery {
        start: None,
        end: None,
        username: None,
        limit: 100,
        offset: 0,
      },
    )
    .await
    .unwrap();
  let mut by_directory: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
  for record in going(&records, Upload).filter(|r| bits(r) == 48 && ended_in(r, window)) {
    by_directory
      .entry(directory(&record.filename))
      .or_default()
      .push(record);
  }
  let mut expected: Vec<(String, u64, u64)> = by_directory
    .into_iter()
    .map(|(directory, rows)| {
      let users = rows
        .iter()
        .map(|r| &r.username)
        .collect::<BTreeSet<_>>()
        .len() as u64;
      (directory, rows.len() as u64, users)
    })
    .collect();
  expected.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0)));
  let found: Vec<(String, u64, u64)> = found
    .into_iter()
    .map(|d| (d.directory, d.count, d.distinct_users))
    .collect();
  assert_eq!(found, expected);
}

#[tokio::test]
async fn the_failures_list_and_count_only_failed_transfers() {
  let records = history();
  let reports = written(&records).await;
  let window = window();
  for direction in [Download, Upload] {
    let failed: Vec<&Record> = going(&records, direction)
      .filter(|r| FAILED.contains(&bits(r)) && ended_in(r, window))
      .collect();
    for sort_order in [SortOrder::Asc, SortOrder::Desc] {
      let query = ExceptionsQuery {
        direction,
        start: None,
        end: None,
        username: None,
        exception: None,
        sort_order,
        limit: 7,
        offset: 2,
      };
      let found = reports.exceptions(window, query).await.unwrap();
      let mut expected = failed.clone();
      expected.sort_by_key(|r| (r.ended_at, r.id.to_string()));
      if sort_order == SortOrder::Desc {
        expected.reverse();
      }
      let wanted: Vec<_> = expected.iter().skip(2).take(7).map(|r| r.id).collect();
      let ids: Vec<_> = found.iter().map(|d| d.id).collect();
      assert_eq!(ids, wanted, "{direction:?} {sort_order:?}");
      assert_eq!(ids.len(), 7, "a full page");
    }
    let mut counts: BTreeMap<Option<&str>, Vec<&Record>> = BTreeMap::new();
    for record in &failed {
      counts
        .entry(record.exception.as_deref())
        .or_default()
        .push(record);
    }
    let mut expected: Vec<(Option<String>, u64, u64)> = counts
      .into_iter()
      .map(|(exception, rows)| {
        let users = rows
          .iter()
          .map(|r| &r.username)
          .collect::<BTreeSet<_>>()
          .len() as u64;
        (exception.map(str::to_string), rows.len() as u64, users)
      })
      .collect();
    expected.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let query = ParetoQuery {
      direction,
      start: None,
      end: None,
      username: None,
      limit: 25,
      offset: 0,
    };
    let found: Vec<(Option<String>, u64, u64)> = reports
      .pareto(window, query)
      .await
      .unwrap()
      .into_iter()
      .map(|c| (c.exception, c.count, c.distinct_users))
      .collect();
    assert_eq!(found, expected, "{direction:?}");
    assert_eq!(found.len(), EXCEPTIONS.len(), "every message, and none");
  }
  let user = reports.user("dave", window).await.unwrap();
  let theirs: Vec<&Record> = going(&records, Download)
    .filter(|r| r.username == "dave" && bits(r) & 16 != 0 && ended_in(r, window))
    .collect();
  same_outcomes(&user.download.summary, &by_outcome(&theirs), "dave");
  let count = |states: &[u32]| theirs.iter().filter(|r| states.contains(&bits(r))).count() as u64;
  assert_eq!(user.download.statistics.total, theirs.len() as u64);
  assert_eq!(user.download.statistics.successful, count(&[48]));
  assert_eq!(
    user.download.statistics.errored,
    count(&[144, 272, 528, 1040])
  );
  assert_eq!(user.download.statistics.cancelled, count(&[80]));
}
