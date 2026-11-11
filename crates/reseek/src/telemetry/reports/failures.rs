//! The transfers that failed, listed by when they ended and counted by their error message. A
//! failed transfer is one that was canceled, timed out, errored, was rejected or aborted; its
//! message is read as stored, which is already without the transfer library's prefix.

use api_types::reports::{ExceptionCount, ExceptionsQuery, ParetoQuery, TransferExceptionDetail};
use api_types::transfers::SortOrder;
use rusqlite::types::{Type, Value};
use rusqlite::{Connection, params_from_iter};

use super::{Filter, Reports, Window};
use crate::db::{self, DbError};
use crate::transfers::categories::FAILED;
use crate::transfers::state::state_at;
use crate::transfers::{direction_at, text_at};

impl Reports {
  /// The failed transfers of one direction, by when they ended.
  pub async fn exceptions(
    &self,
    window: Window,
    query: ExceptionsQuery,
  ) -> Result<Vec<TransferExceptionDetail>, DbError> {
    let filter = Filter::default()
      .ended_between(window)
      .states(&FAILED)
      .direction(Some(query.direction))
      .username(query.username.as_deref())
      .exception(query.exception.as_deref());
    let order = match query.sort_order {
      SortOrder::Asc => "ASC",
      SortOrder::Desc => "DESC",
    };
    let sql = format!(
      "SELECT Id, Username, Direction, Filename, Size, State, RequestedAt, EnqueuedAt, \
       StartedAt, EndedAt, BytesTransferred, AverageSpeed, Exception FROM Transfers{} \
       ORDER BY EndedAt {order}, Id {order} LIMIT ? OFFSET ?",
      filter.clause()
    );
    let mut values = filter.values;
    values.push(Value::Integer(query.limit.into()));
    values.push(Value::Integer(query.offset.into()));
    self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(&sql)?;
        statement
          .query_map(params_from_iter(&values), detail)?
          .collect()
      })
      .await
  }

  /// The failed transfers of one direction counted by error message, most frequent first.
  pub async fn pareto(
    &self,
    window: Window,
    query: ParetoQuery,
  ) -> Result<Vec<ExceptionCount>, DbError> {
    let filter = Filter::default()
      .ended_between(window)
      .direction(Some(query.direction))
      .username(query.username.as_deref());
    self
      .db
      .read(move |conn| pareto_rows(conn, &filter, query.limit, query.offset))
      .await
  }
}

/// The failed transfers among those `filter` selects, counted by error message, most frequent
/// first and then by message.
pub(super) fn pareto_rows(
  conn: &Connection,
  filter: &Filter,
  limit: u32,
  offset: u32,
) -> rusqlite::Result<Vec<ExceptionCount>> {
  let mut conditions = filter.conditions.clone();
  conditions.push(FAILED.to_string());
  let sql = format!(
    "SELECT Exception, COUNT(*) AS Occurrences, COUNT(DISTINCT Username) FROM Transfers \
     WHERE {} GROUP BY Exception ORDER BY Occurrences DESC, Exception ASC LIMIT ? OFFSET ?",
    conditions.join(" AND ")
  );
  let mut values = filter.values.clone();
  values.push(Value::Integer(limit.into()));
  values.push(Value::Integer(offset.into()));
  let mut statement = conn.prepare(&sql)?;
  statement
    .query_map(params_from_iter(&values), |row| {
      Ok(ExceptionCount {
        exception: row.get(0)?,
        count: row.get::<_, i64>(1)?.max(0).unsigned_abs(),
        distinct_users: row.get::<_, i64>(2)?.max(0).unsigned_abs(),
      })
    })?
    .collect()
}

/// A failed transfer from a row of the exceptions query.
fn detail(row: &rusqlite::Row) -> rusqlite::Result<TransferExceptionDetail> {
  let time = |i: usize| -> rusqlite::Result<Option<chrono::DateTime<chrono::Utc>>> {
    row
      .get::<_, Option<String>>(i)?
      .as_deref()
      .map(db::read_timestamp)
      .transpose()
  };
  let required = |i: usize, name: &str| {
    time(i)?.ok_or_else(|| rusqlite::Error::InvalidColumnType(i, name.to_string(), Type::Null))
  };
  let id = row
    .get::<_, String>(0)?
    .parse()
    .map_err(|e: uuid::Error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, e.into()))?;
  let direction = direction_at(row, 2)?;
  Ok(TransferExceptionDetail {
    id,
    username: text_at(row, 1)?,
    direction,
    filename: text_at(row, 3)?,
    size: row.get::<_, i64>(4)?.max(0).unsigned_abs(),
    state: state_at(row, 5)?,
    requested_at: required(6, "RequestedAt")?,
    enqueued_at: time(7)?,
    started_at: time(8)?,
    ended_at: required(9, "EndedAt")?,
    bytes_transferred: row.get::<_, i64>(10)?.max(0).unsigned_abs(),
    average_speed: row.get(11)?,
    exception: row.get(12)?,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::telemetry::reports::tests::{all, at, row};
  use crate::transfers::fixtures::{insert, transfers};
  use api_types::transfers::TransferDirection::Download;
  use api_types::transfers::{TransferOutcome, TransferStates};

  #[tokio::test]
  async fn failures_are_found_by_their_message() {
    let db = transfers().await;
    let failed = |state, minute, message: &str| {
      let mut r = row("a", Download, state, at(10, minute));
      r.exception = Some(message.to_string());
      r
    };
    insert(
      &db,
      vec![
        failed(
          TransferStates::Completed(TransferOutcome::Errored),
          1,
          "Remote file not found: C:\\x",
        ),
        failed(
          TransferStates::Completed(TransferOutcome::Rejected),
          2,
          "File not shared.",
        ),
      ],
    )
    .await;
    let reports = Reports::new(db);
    let query = ExceptionsQuery {
      direction: Download,
      start: None,
      end: None,
      username: None,
      exception: Some("File not shared.".into()),
      sort_order: SortOrder::Asc,
      limit: 25,
      offset: 0,
    };
    let found = reports.exceptions(all(), query).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
      serde_json::to_value(found[0].state).unwrap(),
      "Completed, Rejected"
    );
  }
}
