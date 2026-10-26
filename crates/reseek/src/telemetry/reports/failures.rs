//! The transfers that failed, listed by when they ended and counted by their error message. A
//! failed transfer is one that was cancelled, timed out, errored, was rejected or aborted; its
//! message is read as stored, which is already without the transfer library's prefix.

use api_types::reports::{ExceptionCount, ExceptionsQuery, ParetoQuery, TransferExceptionDetail};
use api_types::transfers::{SortOrder, TransferDirection, TransferStates};
use rusqlite::types::{Type, Value};
use rusqlite::{Connection, params_from_iter};

use super::{FAILED, Filter, Reports};
use crate::db::{self, DbError};

impl Reports {
  /// The failed transfers of one direction, by when they ended.
  pub async fn exceptions(
    &self,
    query: ExceptionsQuery,
  ) -> Result<Vec<TransferExceptionDetail>, DbError> {
    let filter = Filter::default()
      .ended_between(
        query.start.unwrap_or_else(db::earliest),
        query.end.unwrap_or_else(db::latest),
      )
      .states(FAILED)
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
  pub async fn pareto(&self, query: ParetoQuery) -> Result<Vec<ExceptionCount>, DbError> {
    let filter = Filter::default()
      .ended_between(
        query.start.unwrap_or_else(db::earliest),
        query.end.unwrap_or_else(db::latest),
      )
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
  conditions.push(format!("State IN ({FAILED})"));
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
  let direction = match row.get::<_, String>(2)?.as_str() {
    "Upload" => TransferDirection::Upload,
    _ => TransferDirection::Download,
  };
  Ok(TransferExceptionDetail {
    id,
    username: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
    direction,
    filename: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
    size: row.get::<_, i64>(4)?.max(0).unsigned_abs(),
    state: TransferStates::from_bits_retain(row.get(5)?),
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
  use crate::telemetry::reports::tests::{at, row};
  use crate::transfers::fixtures::{insert, transfers};
  use api_types::transfers::TransferDirection::{Download, Upload};

  async fn reports() -> Reports {
    let db = transfers().await;
    let failed = |user: &str, state, minute, message: Option<&str>| {
      let mut r = row(user, Download, state, at(10, minute));
      r.exception = message.map(str::to_string);
      r
    };
    insert(
      &db,
      vec![
        failed("a", 272, 1, Some("Remote file not found: C:\\x")),
        failed("b", 272, 2, Some("Remote file not found: C:\\x")),
        failed("a", 528, 3, Some("File not shared.")),
        failed("a", 80, 4, None),
        failed("a", 16, 5, Some("bare completed")),
        failed("a", 8, 6, Some("still running")),
        failed("a", 48, 7, None),
        row("u", Upload, 272, at(10, 8)),
      ],
    )
    .await;
    Reports::new(db)
  }

  fn exceptions(order: SortOrder) -> ExceptionsQuery {
    ExceptionsQuery {
      direction: Download,
      start: None,
      end: None,
      username: None,
      exception: None,
      sort_order: order,
      limit: 25,
      offset: 0,
    }
  }

  #[tokio::test]
  async fn only_failed_states_are_listed_newest_first_with_messages_intact() {
    let reports = reports().await;
    let listed = reports
      .exceptions(exceptions(SortOrder::Desc))
      .await
      .unwrap();
    let minutes: Vec<_> = listed
      .iter()
      .map(|d| d.ended_at.format("%M").to_string())
      .collect();
    assert_eq!(
      minutes,
      ["04", "03", "02", "01"],
      "not bare Completed, not running, not succeeded, not the upload"
    );
    assert_eq!(
      listed[3].exception.as_deref(),
      Some("Remote file not found: C:\\x"),
      "a message with a colon is kept whole"
    );
    let mut by_message = exceptions(SortOrder::Asc);
    by_message.exception = Some("File not shared.".into());
    let found = reports.exceptions(by_message).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
      serde_json::to_value(found[0].state).unwrap(),
      "Completed, Rejected"
    );
  }

  #[tokio::test]
  async fn the_pareto_counts_messages_most_frequent_first() {
    let counted = reports()
      .await
      .pareto(ParetoQuery {
        direction: Download,
        start: None,
        end: None,
        username: None,
        limit: 25,
        offset: 0,
      })
      .await
      .unwrap();
    let rows: Vec<_> = counted
      .iter()
      .map(|c| (c.exception.as_deref(), c.count, c.distinct_users))
      .collect();
    assert_eq!(
      rows,
      [
        (Some("Remote file not found: C:\\x"), 2, 2),
        (None, 1, 1),
        (Some("File not shared."), 1, 1)
      ]
    );
  }
}
