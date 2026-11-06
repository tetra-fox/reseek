//! Transfers: the database every download and upload is recorded in, reading a record back as
//! the API shows it, and what both directions share: writing records, the listings, removal and
//! retention. The engines are [`downloads`] and [`uploads`]; [`service::Transfers`] answers
//! the API with both.

pub mod categories;
pub mod cleanup;
pub mod downloads;
#[cfg(test)]
pub mod fixtures;
pub mod listing;
pub mod prune;
pub mod record;
pub mod removal;
pub mod service;
pub mod speed;
pub mod store;
pub mod uploads;

use api_types::transfers::TransferDirection;
use rusqlite::Row;
use rusqlite::types::Type;

use crate::db::{self, Database, DbError, Location, Schema};
pub use record::{COLUMNS, Record, transfer};

/// `transfers.db`: the `Transfers` table, each transfer's state history, and which speed limits
/// are in force.
pub static SCHEMA: Schema = db::schema!(
  "transfers",
  [
    "0001_initial",
    "0002_jobs",
    "0003_fold_batches",
    "0004_speed_limits",
    "0005_paused",
  ]
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
