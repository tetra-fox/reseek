//! Finished transfers by time bucket. Buckets are aligned to whole multiples of their width
//! since the Unix epoch, so a day-wide bucket starts at midnight UTC, and every bucket of the
//! window is listed, empty or not.

use std::collections::BTreeMap;

use api_types::reports::{DirectionSummaries, HistogramQuery, TransferSummary};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params_from_iter};

use super::{Filter, Reports, outcome};
use crate::db::{self, DbError};
use crate::transfers::categories::COMPLETED;

/// The first release; a window that starts earlier starts at the oldest transfer instead.
const GENESIS: &str = "2020-12-30T06:22:00Z";

/// The most buckets a histogram has; a narrower interval over a wider window is refused.
pub const MOST_BUCKETS: i64 = 100_000;

/// What a histogram could not be made of.
#[derive(Debug, thiserror::Error)]
pub enum HistogramError {
  /// the interval cuts the window into more than [`MOST_BUCKETS`] buckets
  #[error("the interval cuts the window into {0} buckets, more than {MOST_BUCKETS}")]
  TooManyBuckets(i64),
  /// the database failed
  #[error(transparent)]
  Db(#[from] DbError),
}

impl Reports {
  /// The window of `query` cut into buckets of `interval` minutes, or into `buckets` of equal
  /// whole seconds, each with its finished transfers by direction and outcome. A bucket's
  /// `averageSpeed` is its bytes over the seconds its transfers took.
  pub async fn histogram(
    &self,
    query: HistogramQuery,
  ) -> Result<BTreeMap<String, DirectionSummaries>, HistogramError> {
    let genesis: DateTime<Utc> = GENESIS.parse().expect("the first release is RFC 3339");
    let end = query.end.unwrap_or_else(Utc::now);
    let mut start = query.start.unwrap_or(genesis);
    if start < genesis {
      let oldest: Option<String> = self
        .db
        .read(|conn| {
          conn
            .query_row("SELECT MIN(RequestedAt) FROM Transfers", [], |row| {
              row.get(0)
            })
            .optional()
            .map(Option::flatten)
        })
        .await?;
      start = match oldest {
        Some(text) => db::read_timestamp(&text).map_err(DbError::from)?,
        None => genesis,
      };
    }
    let width = match (query.interval, query.buckets) {
      (Some(minutes), _) => i64::from(minutes) * 60,
      (None, buckets) => {
        let seconds = (end - start).num_seconds().max(0);
        (seconds / i64::from(buckets.unwrap_or(100).max(1))).max(1)
      }
    };
    let first = start.timestamp().div_euclid(width) * width;
    let end_seconds = end.timestamp() + i64::from(end.timestamp_subsec_nanos() > 0);
    let count = (end_seconds - first + width - 1).div_euclid(width).max(0);
    if count > MOST_BUCKETS {
      return Err(HistogramError::TooManyBuckets(count));
    }
    let mut result = BTreeMap::new();
    let key = |seconds: i64| {
      DateTime::<Utc>::from_timestamp(seconds, 0)
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
    };
    for bucket in 0..count {
      result.insert(key(first + bucket * width), DirectionSummaries::default());
    }
    let filter = Filter::default()
      .ended_from(start, end)
      .states(COMPLETED)
      .direction(query.direction)
      .username(query.username.as_deref());
    let rows = self
      .db
      .read(move |conn| {
        let sql = format!(
          "SELECT (unixepoch(EndedAt) - ((unixepoch(EndedAt) % ?1) + ?1) % ?1) AS Bucket, \
           Direction, State, COALESCE(SUM(Size), 0), COUNT(*), COUNT(DISTINCT Username), \
           COALESCE(SUM(Size) * 1.0 / NULLIF(SUM(unixepoch(EndedAt) - unixepoch(StartedAt)), 0), 0), \
           COALESCE(AVG(unixepoch(StartedAt) - unixepoch(EnqueuedAt)), 0), \
           COALESCE(AVG(unixepoch(EndedAt) - unixepoch(StartedAt)), 0) \
           FROM Transfers{} GROUP BY Bucket, Direction, State",
          // the width is ?1, so the filter's placeholders take the numbers after it
          filter.clause()
        );
        let mut values = vec![rusqlite::types::Value::Integer(width)];
        values.extend(filter.values.iter().cloned());
        let mut statement = conn.prepare(&sql)?;
        statement
          .query_map(params_from_iter(&values), |row| {
            Ok((
              row.get::<_, i64>(0)?,
              row.get::<_, String>(1)?,
              row.get::<_, u32>(2)?,
              TransferSummary {
                total_bytes: row.get::<_, i64>(3)?.max(0).unsigned_abs(),
                count: row.get::<_, i64>(4)?.max(0).unsigned_abs(),
                distinct_users: row.get::<_, i64>(5)?.max(0).unsigned_abs(),
                average_speed: row.get(6)?,
                average_wait: row.get(7)?,
                average_duration: row.get(8)?,
              },
            ))
          })?
          .collect::<rusqlite::Result<Vec<_>>>()
      })
      .await?;
    for (bucket, direction, state, summary) in rows {
      // with the end excluded every row ends in a listed bucket; one that does not is dropped
      let Some(summaries) = result.get_mut(&key(bucket)) else {
        continue;
      };
      let into = match direction.as_str() {
        "Download" => &mut summaries.download,
        "Upload" => &mut summaries.upload,
        _ => continue,
      };
      into.insert(outcome(state), summary);
    }
    Ok(result)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::telemetry::reports::tests::{at, row};
  use crate::transfers::fixtures::{insert, transfers};
  use api_types::reports::TransferOutcome;
  use api_types::transfers::TransferDirection::{Download, Upload};

  fn query(start: DateTime<Utc>, end: DateTime<Utc>) -> HistogramQuery {
    HistogramQuery {
      start: Some(start),
      end: Some(end),
      ..HistogramQuery::default()
    }
  }

  #[tokio::test]
  async fn buckets_are_epoch_aligned_and_the_end_is_excluded() {
    let db = transfers().await;
    let mut slow = row("alice", Upload, 48, at(1, 30));
    slow.size = 600;
    slow.started_at = Some(at(1, 20));
    let mut fast = row("bob", Upload, 48, at(1, 40));
    fast.size = 600;
    fast.started_at = Some(at(1, 38));
    let on_the_end = row("carol", Download, 48, at(3, 0));
    let failed = row("dave", Download, 272, at(2, 59));
    insert(&db, vec![slow, fast, on_the_end, failed]).await;
    let reports = Reports::new(db);
    let mut q = query(at(0, 30), at(3, 0));
    q.interval = Some(60);
    let histogram = reports.histogram(q).await.unwrap();
    let keys: Vec<_> = histogram.keys().cloned().collect();
    assert_eq!(
      keys,
      [
        "2026-03-01T00:00:00Z",
        "2026-03-01T01:00:00Z",
        "2026-03-01T02:00:00Z"
      ],
      "the first bucket starts before the window, the last ends at it"
    );
    let one = &histogram["2026-03-01T01:00:00Z"].upload[&TransferOutcome::Succeeded];
    assert_eq!(
      (one.count, one.total_bytes, one.distinct_users),
      (2, 1200, 2)
    );
    // 1200 bytes over 600 + 120 seconds of transfer
    assert!((one.average_speed - 1200.0 / 720.0).abs() < 1e-9);
    let two = &histogram["2026-03-01T02:00:00Z"];
    assert_eq!(two.download[&TransferOutcome::Errored].count, 1);
    assert!(
      !two.download.contains_key(&TransferOutcome::Succeeded),
      "a transfer ending on the window's end is outside it"
    );
    assert!(histogram["2026-03-01T00:00:00Z"].upload.is_empty());
  }

  #[tokio::test]
  async fn a_bucket_count_divides_the_window_in_whole_seconds() {
    let reports = Reports::new(transfers().await);
    let mut q = query(at(0, 0), at(0, 0) + chrono::Duration::days(7));
    q.buckets = Some(84);
    let histogram = reports.histogram(q).await.unwrap();
    assert_eq!(histogram.len(), 84);
    let mut keys = histogram.keys();
    keys.next();
    assert_eq!(
      keys.next().unwrap(),
      "2026-03-01T02:00:00Z",
      "7 days / 84 is 2 hours"
    );
    let mut odd = query(at(0, 7), at(0, 7) + chrono::Duration::days(1));
    odd.buckets = Some(24);
    assert_eq!(
      reports.histogram(odd).await.unwrap().len(),
      25,
      "a start between bucket starts adds one"
    );
  }

  #[tokio::test]
  async fn a_window_before_the_first_release_starts_at_the_oldest_transfer() {
    let db = transfers().await;
    let mut old = row("alice", Upload, 48, at(5, 0));
    old.requested_at = at(4, 10);
    insert(&db, vec![old]).await;
    let reports = Reports::new(db);
    let mut q = query("1970-01-01T00:00:00Z".parse().unwrap(), at(6, 0));
    q.interval = Some(60);
    let histogram = reports.histogram(q).await.unwrap();
    assert_eq!(histogram.keys().next().unwrap(), "2026-03-01T04:00:00Z");
    assert_eq!(histogram.len(), 2);
    let mut tiny = query(at(0, 0), at(0, 0) + chrono::Duration::days(365));
    tiny.interval = Some(1);
    assert!(matches!(
      reports.histogram(tiny).await,
      Err(HistogramError::TooManyBuckets(_))
    ));
  }
}
