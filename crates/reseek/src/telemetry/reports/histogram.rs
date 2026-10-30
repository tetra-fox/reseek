//! Finished transfers by time bucket. Buckets are aligned to whole multiples of their width
//! since the Unix epoch, so a day-wide bucket starts at midnight UTC, and every bucket of the
//! window is listed, empty or not.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use api_types::reports::{DirectionSummaries, HistogramQuery, TransferSummary};
use api_types::transfers::TransferDirection;
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params_from_iter};

use super::{Filter, Reports, Window, genesis, outcome};
use crate::db::{self, DbError};
use crate::transfers::categories::COMPLETED;
use crate::transfers::direction_at;

/// How wide a histogram's buckets are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
  /// this many minutes each
  Minutes(NonZeroU32),
  /// the window cut into this many buckets of equal whole seconds, at least one each
  Buckets(NonZeroU32),
}

/// The most buckets a histogram has; a narrower interval over a wider window is refused.
pub const MOST_BUCKETS: i64 = 100_000;

/// What a histogram could not be made of.
#[derive(Debug, thiserror::Error)]
pub enum HistogramError {
  /// the interval cuts the window into more than `MOST_BUCKETS` buckets
  #[error("the interval cuts the window into {0} buckets, more than {MOST_BUCKETS}")]
  TooManyBuckets(i64),
  /// the database failed
  #[error(transparent)]
  Db(#[from] DbError),
}

impl Reports {
  /// `window` cut into buckets of `width`, each with the finished transfers `query` selects by
  /// direction and outcome; a window starting before the first release starts at the oldest
  /// transfer. A bucket's `averageSpeed` is its bytes over the seconds its transfers took.
  pub async fn histogram(
    &self,
    window: Window,
    width: Width,
    query: HistogramQuery,
  ) -> Result<BTreeMap<String, DirectionSummaries>, HistogramError> {
    let genesis = genesis();
    let end = window.end;
    let mut start = window.start;
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
    let width = match width {
      Width::Minutes(minutes) => i64::from(minutes.get()) * 60,
      Width::Buckets(buckets) => {
        let seconds = (end - start).num_seconds().max(0);
        (seconds / i64::from(buckets.get())).max(1)
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
        .expect("a bucket lies between the years the databases store")
        .to_rfc3339_opts(SecondsFormat::Secs, true)
    };
    for bucket in 0..count {
      result.insert(key(first + bucket * width), DirectionSummaries::default());
    }
    let filter = Filter::default()
      .ended_from(Window { start, end })
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
              direction_at(row, 1)?,
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
      let into = match direction {
        TransferDirection::Download => &mut summaries.download,
        TransferDirection::Upload => &mut summaries.upload,
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
  use api_types::transfers::TransferDirection::Upload;

  async fn cut(
    reports: &Reports,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    width: Width,
  ) -> Result<BTreeMap<String, DirectionSummaries>, HistogramError> {
    reports
      .histogram(Window { start, end }, width, HistogramQuery::default())
      .await
  }

  #[tokio::test]
  async fn a_bucket_count_divides_the_window_in_whole_seconds() {
    let reports = Reports::new(transfers().await);
    let week = at(0, 0) + chrono::Duration::days(7);
    let histogram = cut(
      &reports,
      at(0, 0),
      week,
      Width::Buckets(NonZeroU32::new(84).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(histogram.len(), 84);
    let mut keys = histogram.keys();
    keys.next();
    assert_eq!(
      keys.next().unwrap(),
      "2026-03-01T02:00:00Z",
      "7 days / 84 is 2 hours"
    );
    let day = at(0, 7) + chrono::Duration::days(1);
    assert_eq!(
      cut(
        &reports,
        at(0, 7),
        day,
        Width::Buckets(NonZeroU32::new(24).unwrap())
      )
      .await
      .unwrap()
      .len(),
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
    let epoch = "1970-01-01T00:00:00Z".parse().unwrap();
    let histogram = cut(
      &reports,
      epoch,
      at(6, 0),
      Width::Minutes(NonZeroU32::new(60).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(histogram.keys().next().unwrap(), "2026-03-01T04:00:00Z");
    assert_eq!(histogram.len(), 2);
    let year = at(0, 0) + chrono::Duration::days(365);
    assert!(matches!(
      cut(
        &reports,
        at(0, 0),
        year,
        Width::Minutes(NonZeroU32::new(1).unwrap())
      )
      .await,
      Err(HistogramError::TooManyBuckets(_))
    ));
  }
}
