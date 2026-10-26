//! Finished transfers by direction and outcome, over everyone or for one user.

use std::collections::BTreeMap;

use api_types::reports::{
  DirectionSummaries, SummaryQuery, SummaryReport, TransferOutcome, TransferSummary,
  UserDirectionReport, UserReport, UserTransferStatistics,
};
use api_types::transfers::TransferDirection;
use rusqlite::{Connection, params_from_iter};

use super::failures::pareto_rows;
use super::{Filter, Reports, SUMMARY, Window, outcome, summary_at};
use crate::db::DbError;
use crate::transfers::categories::{COMPLETED, SUCCESSFUL};
use crate::transfers::direction_at;

/// How many error messages a user report lists.
const USER_EXCEPTIONS: u32 = 25;

impl Reports {
  /// Finished transfers in the window by direction and outcome, with how many distinct users
  /// we transferred with successfully.
  pub async fn summary(
    &self,
    window: Window,
    query: SummaryQuery,
  ) -> Result<SummaryReport, DbError> {
    self
      .db
      .read(move |conn| {
        let filter = || {
          Filter::default()
            .ended_between(window)
            .direction(query.direction)
            .username(query.username.as_deref())
        };
        let directions = by_outcome(conn, filter().states(COMPLETED))?;
        let succeeded = filter().states(SUCCESSFUL);
        let distinct_users: i64 = conn.query_row(
          &format!(
            "SELECT COUNT(DISTINCT Username) FROM Transfers{}",
            succeeded.clause()
          ),
          params_from_iter(&succeeded.values),
          |row| row.get(0),
        )?;
        Ok(SummaryReport {
          directions,
          distinct_users: distinct_users.unsigned_abs(),
        })
      })
      .await
  }

  /// One user's finished transfers in the window, each direction with its outcomes, their
  /// counts and the most frequent error messages.
  pub async fn user(&self, username: &str, window: Window) -> Result<UserReport, DbError> {
    let username = username.to_string();
    self
      .db
      .read(move |conn| {
        let direction = |direction| -> rusqlite::Result<UserDirectionReport> {
          let filter = || {
            Filter::default()
              .ended_between(window)
              .direction(Some(direction))
              .username(Some(&username))
          };
          let summaries = by_outcome(conn, filter().states(COMPLETED))?;
          let summary = match direction {
            TransferDirection::Download => summaries.download,
            TransferDirection::Upload => summaries.upload,
          };
          let exceptions = pareto_rows(conn, &filter(), USER_EXCEPTIONS, 0)?;
          Ok(UserDirectionReport {
            statistics: statistics(&summary),
            summary,
            exceptions,
          })
        };
        Ok(UserReport {
          download: direction(TransferDirection::Download)?,
          upload: direction(TransferDirection::Upload)?,
        })
      })
      .await
  }
}

/// The transfers `filter` selects, summarised by direction and outcome.
fn by_outcome(conn: &Connection, filter: Filter) -> rusqlite::Result<DirectionSummaries> {
  let sql = format!(
    "SELECT Direction, State, {SUMMARY} FROM Transfers{} GROUP BY Direction, State",
    filter.clause()
  );
  let mut statement = conn.prepare(&sql)?;
  let rows = statement.query_map(params_from_iter(&filter.values), |row| {
    Ok((
      direction_at(row, 0)?,
      row.get::<_, u32>(1)?,
      summary_at(row, 2)?,
    ))
  })?;
  let mut summaries = DirectionSummaries::default();
  for row in rows {
    let (direction, state, summary) = row?;
    let into = match direction {
      TransferDirection::Download => &mut summaries.download,
      TransferDirection::Upload => &mut summaries.upload,
    };
    into.insert(outcome(state), summary);
  }
  Ok(summaries)
}

/// The outcome counts of one direction's summary.
fn statistics(summary: &BTreeMap<TransferOutcome, TransferSummary>) -> UserTransferStatistics {
  let count = |outcome| summary.get(&outcome).map_or(0, |s| s.count);
  UserTransferStatistics {
    total: summary.values().map(|s| s.count).sum(),
    successful: count(TransferOutcome::Succeeded),
    errored: [
      TransferOutcome::Errored,
      TransferOutcome::TimedOut,
      TransferOutcome::Rejected,
      TransferOutcome::Aborted,
    ]
    .into_iter()
    .map(count)
    .sum(),
    cancelled: count(TransferOutcome::Cancelled),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::telemetry::reports::tests::{all, at, fixture_reports};

  #[tokio::test]
  async fn the_summary_counts_finished_transfers_by_outcome_within_both_ends() {
    let reports = fixture_reports().await;
    let report = reports
      .summary(
        Window {
          start: at(10, 0),
          end: at(12, 0),
        },
        SummaryQuery::default(),
      )
      .await
      .unwrap();
    let up = &report.directions.upload[&TransferOutcome::Succeeded];
    // alice 10:00 and bob 12:00 sit on the window's ends and count; carol 09:00 does not
    assert_eq!((up.count, up.total_bytes, up.distinct_users), (2, 300, 2));
    assert_eq!(up.average_speed, 150.0);
    assert_eq!(up.average_wait, 30.0);
    assert_eq!(up.average_duration, 60.0);
    let failed = &report.directions.download[&TransferOutcome::Errored];
    assert_eq!(failed.count, 1);
    assert!(
      !report
        .directions
        .download
        .contains_key(&TransferOutcome::Succeeded),
      "only outcomes that happened"
    );
    assert_eq!(report.distinct_users, 2, "alice and bob succeeded");
    let only_dave = reports
      .summary(
        all(),
        SummaryQuery {
          direction: Some(TransferDirection::Download),
          username: Some("dave".into()),
          ..SummaryQuery::default()
        },
      )
      .await
      .unwrap();
    assert!(only_dave.directions.upload.is_empty());
    assert_eq!(only_dave.directions.download.len(), 2);
  }

  #[tokio::test]
  async fn a_user_report_counts_outcomes_and_lists_the_errors() {
    let reports = fixture_reports().await;
    let report = reports.user("dave", all()).await.unwrap();
    assert_eq!(
      report.download.statistics,
      UserTransferStatistics {
        total: 2,
        successful: 0,
        errored: 1,
        cancelled: 1
      }
    );
    assert_eq!(report.download.exceptions.len(), 2);
    assert_eq!(report.upload.statistics.total, 0);
  }
}
