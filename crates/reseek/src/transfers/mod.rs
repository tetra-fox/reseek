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
pub use record::{ASKED_ORDER, COLUMNS, NEWEST_FIRST, Record, transfer};

/// `transfers.db`: the `Transfers` table, each transfer's state history, and which speed limits
/// are in force.
pub static SCHEMA: Schema = db::schema!("transfers", ["0001_initial", "0002_queue_position"]);

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
  use super::*;
  use crate::transfers::fixtures::{Row, insert};

  /// `transfers.db` before the local queue's order was kept.
  static BEFORE_QUEUE_POSITIONS: Schema = db::schema!("transfers", ["0001_initial"]);

  #[tokio::test]
  async fn downloads_written_before_places_were_kept_are_placed_in_the_order_they_were_asked() {
    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    let old = Database::open(&BEFORE_QUEUE_POSITIONS, &location)
      .await
      .unwrap();
    let at = |minute: u32| format!("2026-11-01T10:0{minute}:00Z").parse().unwrap();
    let later = Row {
      requested_at: at(2),
      ..Row::new("alice", TransferDirection::Download, 2 | 4096)
    };
    let earlier = Row {
      requested_at: at(1),
      ..Row::new("alice", TransferDirection::Download, 2 | 4096)
    };
    let upload = Row::new("bob", TransferDirection::Upload, 2);
    let ids = [later.id, earlier.id, upload.id];
    insert(&old, vec![later, earlier, upload]).await;
    drop(old);
    let migrated = open(&location).await.unwrap();
    let (placed, next) = migrated
      .read(move |conn| {
        let placed = ids
          .iter()
          .map(|id| store::get(conn, *id).map(|record| record.unwrap().queue_position))
          .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((placed, store::next_position(conn)?))
      })
      .await
      .unwrap();
    assert_eq!(placed, [Some(2.0), Some(1.0), None]);
    assert_eq!(next, 3.0);
  }
}
