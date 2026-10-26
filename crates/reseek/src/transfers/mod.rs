//! Transfers: the database every download and upload is recorded in, reading a record back as
//! the API shows it, and what both directions share: writing records, the listings, removal and
//! retention.

pub mod categories;
pub mod cleanup;
#[cfg(test)]
pub mod fixtures;
pub mod listing;
pub mod prune;
pub mod record;
pub mod removal;
pub mod store;

use api_types::transfers::TransferDirection;
use rusqlite::Row;
use rusqlite::types::Type;

use crate::db::{self, Database, DbError, Location, Schema};
pub use record::{COLUMNS, Record, transfer};

/// `transfers.db`: the `Transfers` and `Batches` tables and each transfer's state history.
pub static SCHEMA: Schema = db::schema!(
  "transfers",
  ["0001_initial", "0002_local_path_history_files"]
);

/// Opens `transfers.db` in `location`.
pub async fn open(location: &Location) -> Result<Database, DbError> {
  Database::open(&SCHEMA, location).await
}

/// A direction as the database stores it.
pub fn direction_text(direction: TransferDirection) -> &'static str {
  match direction {
    TransferDirection::Download => "Download",
    TransferDirection::Upload => "Upload",
  }
}

/// The text in the nullable column `i` of `row`, such as `Username` or `Filename`, empty for
/// NULL: the transfer engines always write them, so a NULL is a row from elsewhere and reads
/// as no name.
pub fn text_at(row: &Row, i: usize) -> rusqlite::Result<String> {
  Ok(row.get::<_, Option<String>>(i)?.unwrap_or_default())
}

/// The direction in the column `i` of `row`, as [`direction_text`] wrote it.
pub fn direction_at(row: &Row, i: usize) -> rusqlite::Result<TransferDirection> {
  match row.get_ref(i)?.as_str()? {
    "Download" => Ok(TransferDirection::Download),
    "Upload" => Ok(TransferDirection::Upload),
    other => Err(rusqlite::Error::FromSqlConversionFailure(
      i,
      Type::Text,
      format!("{other} is no direction").into(),
    )),
  }
}

#[cfg(test)]
mod tests {
  use super::fixtures::transfers;

  #[tokio::test]
  async fn a_fresh_database_has_every_column_and_table_of_the_migrations() {
    let db = transfers().await;
    let (columns, history, files): (Vec<String>, i64, i64) = db
      .read(|conn| {
        let columns = conn
          .prepare("SELECT name FROM pragma_table_info('Transfers')")?
          .query_map([], |row| row.get(0))?
          .collect::<rusqlite::Result<_>>()?;
        let history = conn.query_row(
          "SELECT COUNT(*) FROM sqlite_master WHERE name = 'transfers_state_history'",
          [],
          |row| row.get(0),
        )?;
        let files = conn.query_row(
          "SELECT COUNT(*) FROM pragma_table_info('Batches') WHERE name = 'files_json'",
          [],
          |row| row.get(0),
        )?;
        Ok((columns, history, files))
      })
      .await
      .unwrap();
    for column in [
      "LocalPath",
      "RemovedAt",
      "BatchId",
      "Attempts",
      "NextAttemptAt",
    ] {
      assert!(columns.iter().any(|c| c == column), "{column}: {columns:?}");
    }
    assert_eq!((history, files), (1, 1));
  }
}
