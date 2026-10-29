//! Transfer rows written straight into a test database, for the reports and the users
//! directory that read them.

use api_types::transfers::{TransferDirection, TransferStates};
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

use super::direction_text;
use crate::db::{Database, timestamp};

/// One row of `Transfers`, everything but the user, direction and state defaulted.
#[derive(Clone, Debug)]
pub struct Row {
  /// the id, random
  pub id: Uuid,
  /// the peer
  pub username: String,
  /// which way
  pub direction: TransferDirection,
  /// the remote filename, `@@share\dir\file.mp3`
  pub filename: String,
  /// bytes, 100
  pub size: i64,
  /// the state bits
  pub state: u32,
  /// 2026-01-01
  pub requested_at: DateTime<Utc>,
  /// none
  pub enqueued_at: Option<DateTime<Utc>>,
  /// none
  pub started_at: Option<DateTime<Utc>>,
  /// none
  pub ended_at: Option<DateTime<Utc>>,
  /// bytes moved, 0
  pub bytes: i64,
  /// bytes per second, 0
  pub speed: f64,
  /// none
  pub exception: Option<String>,
  /// false
  pub removed: bool,
}

impl Row {
  /// A row of `username` going `direction` in the state with the bits `state`.
  pub fn new(username: &str, direction: TransferDirection, state: u32) -> Self {
    Self {
      id: Uuid::new_v4(),
      username: username.to_string(),
      direction,
      filename: "@@share\\dir\\file.mp3".to_string(),
      size: 100,
      state,
      requested_at: "2026-01-01T00:00:00Z".parse().unwrap(),
      enqueued_at: None,
      started_at: None,
      ended_at: None,
      bytes: 0,
      speed: 0.0,
      exception: None,
      removed: false,
    }
  }
}

/// An empty `transfers.db` in memory.
pub async fn transfers() -> Database {
  Database::memory(&super::SCHEMA).await
}

/// Writes `rows` into `db`.
pub async fn insert(db: &Database, rows: Vec<Row>) {
  db.write(move |conn| {
    let tx = conn.transaction()?;
    for row in &rows {
      let time = |t: Option<DateTime<Utc>>| t.map(timestamp);
      tx.execute(
        "INSERT INTO Transfers (Id, Username, Direction, Filename, Size, State, \
         StateDescription, RequestedAt, EnqueuedAt, StartedAt, EndedAt, BytesTransferred, \
         AverageSpeed, Exception, Removed) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
          row.id.to_string(),
          row.username,
          direction_text(row.direction),
          row.filename,
          row.size,
          row.state,
          serde_json::to_value(TransferStates::from_bits_retain(row.state))
            .unwrap()
            .as_str()
            .unwrap()
            .to_string(),
          timestamp(row.requested_at),
          time(row.enqueued_at),
          time(row.started_at),
          time(row.ended_at),
          row.bytes,
          row.speed,
          row.exception,
          row.removed,
        ],
      )?;
    }
    tx.commit()
  })
  .await
  .unwrap();
}
