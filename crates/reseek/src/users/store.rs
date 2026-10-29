//! `users.db`: the row kept for every user we dealt with, read and written.

use api_types::state::UserStatistics;
use api_types::users::UserPresence;
use chrono::{DateTime, Utc};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::{self, Schema, read_timestamp, timestamp};

/// `users.db`: the `users` and `blocks` tables.
pub static SCHEMA: Schema = db::schema!("users", ["0001_initial"]);

/// What the peer last said about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
  /// its free text
  pub description: String,
  /// it sent a picture
  pub has_picture: bool,
  /// its upload slots
  pub upload_slots: u32,
  /// how many uploads it had queued
  pub queue_length: u32,
  /// it had an upload slot free
  pub has_free_slot: bool,
  /// when it said so
  pub at: DateTime<Utc>,
}

/// A user's row, without the picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
  /// the user
  pub username: String,
  /// when we first dealt with them
  pub first_seen_at: DateTime<Utc>,
  /// when we last dealt with them
  pub last_seen_at: DateTime<Utc>,
  /// the presence last observed, with when
  pub presence: Option<(UserPresence, Option<DateTime<Utc>>)>,
  /// the server last called them privileged
  pub is_privileged: bool,
  /// the statistics last fetched, with when
  pub statistics: Option<(UserStatistics, DateTime<Utc>)>,
  /// their country from a room member list
  pub country_code: Option<String>,
  /// what they last said about themselves
  pub info: Option<Info>,
  /// our notes
  pub notes: String,
  /// our labels
  pub tags: Vec<String>,
  /// the user-defined group we put them in
  pub group_override: Option<String>,
  /// their notifications are off
  pub muted: bool,
  /// listed first
  pub pinned: bool,
}

/// The columns [`stored`] reads, in its order.
pub const COLUMNS: &str = "username, first_seen_at, last_seen_at, last_presence, presence_at, \
                           is_privileged, average_speed, upload_count, file_count, \
                           directory_count, statistics_at, country_code, description, \
                           has_picture, upload_slots, queue_length, has_free_slot, info_at, \
                           notes, tags, group_override, muted, pinned";

/// Every user we put in a user-defined group, with the group.
pub fn overrides(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
  conn
    .prepare("SELECT username, group_override FROM users WHERE group_override IS NOT NULL")?
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
    .collect()
}

/// A presence as stored.
pub fn presence_text(presence: UserPresence) -> &'static str {
  match presence {
    UserPresence::Online => "Online",
    UserPresence::Away => "Away",
    UserPresence::Offline => "Offline",
  }
}

fn conversion(column: usize, message: String) -> rusqlite::Error {
  rusqlite::Error::FromSqlConversionFailure(column, Type::Text, message.into())
}

fn time(row: &Row, column: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
  row
    .get::<_, Option<String>>(column)?
    .as_deref()
    .map(read_timestamp)
    .transpose()
}

/// The row a `SELECT` of [`COLUMNS`] gave.
pub fn stored(row: &Row) -> rusqlite::Result<Stored> {
  let presence = match row.get::<_, Option<String>>(3)?.as_deref() {
    None => None,
    Some("Online") => Some(UserPresence::Online),
    Some("Away") => Some(UserPresence::Away),
    Some("Offline") => Some(UserPresence::Offline),
    Some(other) => return Err(conversion(3, format!("{other} is no presence"))),
  };
  let statistics = match time(row, 10)? {
    Some(at) => Some((
      UserStatistics {
        average_speed: row.get::<_, Option<u32>>(6)?.unwrap_or(0),
        upload_count: row
          .get::<_, Option<i64>>(7)?
          .unwrap_or(0)
          .max(0)
          .unsigned_abs(),
        file_count: row.get::<_, Option<u32>>(8)?.unwrap_or(0),
        directory_count: row.get::<_, Option<u32>>(9)?.unwrap_or(0),
      },
      at,
    )),
    None => None,
  };
  let info = match time(row, 17)? {
    Some(at) => Some(Info {
      description: row.get::<_, Option<String>>(12)?.unwrap_or_default(),
      has_picture: row.get::<_, Option<bool>>(13)?.unwrap_or(false),
      upload_slots: row.get::<_, Option<u32>>(14)?.unwrap_or(0),
      queue_length: row.get::<_, Option<u32>>(15)?.unwrap_or(0),
      has_free_slot: row.get::<_, Option<bool>>(16)?.unwrap_or(false),
      at,
    }),
    None => None,
  };
  let tags: String = row.get(19)?;
  Ok(Stored {
    username: row.get(0)?,
    first_seen_at: read_timestamp(&row.get::<_, String>(1)?)?,
    last_seen_at: read_timestamp(&row.get::<_, String>(2)?)?,
    presence: presence
      .map(|p| Ok::<_, rusqlite::Error>((p, time(row, 4)?)))
      .transpose()?,
    is_privileged: row.get(5)?,
    statistics,
    country_code: row.get(11)?,
    info,
    notes: row.get(18)?,
    tags: serde_json::from_str(&tags).map_err(|e| conversion(19, e.to_string()))?,
    group_override: row.get(20)?,
    muted: row.get(21)?,
    pinned: row.get(22)?,
  })
}

/// The row of `username`, if there is one.
pub fn get(conn: &Connection, username: &str) -> rusqlite::Result<Option<Stored>> {
  conn
    .query_row(
      &format!("SELECT {COLUMNS} FROM users WHERE username = ?1"),
      [username],
      stored,
    )
    .optional()
}

/// Makes sure `username` has a row, seen at `now`.
pub fn ensure(conn: &Connection, username: &str, now: DateTime<Utc>) -> rusqlite::Result<()> {
  let now = timestamp(now);
  conn.execute(
    "INSERT INTO users (username, first_seen_at, last_seen_at) VALUES (?1, ?2, ?2) \
     ON CONFLICT (username) DO UPDATE SET last_seen_at = excluded.last_seen_at",
    params![username, now],
  )?;
  Ok(())
}

/// What a write through the API sets on a user's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
  /// our notes
  Notes(String),
  /// our labels, lower case and unique
  Tags(Vec<String>),
  /// listed first or not
  Pinned(bool),
  /// notifications off or on
  Muted(bool),
  /// the user-defined group, or none
  Group(Option<String>),
}

/// Applies `change` to `username`'s row, making the row when there is none.
pub fn change(
  conn: &mut Connection,
  username: &str,
  change: &Change,
  now: DateTime<Utc>,
) -> rusqlite::Result<()> {
  let tx = conn.transaction()?;
  ensure(&tx, username, now)?;
  match change {
    Change::Notes(notes) => tx.execute(
      "UPDATE users SET notes = ?2 WHERE username = ?1",
      params![username, notes],
    )?,
    Change::Tags(tags) => tx.execute(
      "UPDATE users SET tags = ?2 WHERE username = ?1",
      params![
        username,
        serde_json::to_string(tags).expect("strings are JSON")
      ],
    )?,
    Change::Pinned(pinned) => tx.execute(
      "UPDATE users SET pinned = ?2 WHERE username = ?1",
      params![username, pinned],
    )?,
    Change::Muted(muted) => tx.execute(
      "UPDATE users SET muted = ?2 WHERE username = ?1",
      params![username, muted],
    )?,
    Change::Group(group) => tx.execute(
      "UPDATE users SET group_override = ?2 WHERE username = ?1",
      params![username, group],
    )?,
  };
  tx.commit()
}

/// The picture stored for `username`, if any.
pub fn picture(conn: &Connection, username: &str) -> rusqlite::Result<Option<Vec<u8>>> {
  Ok(
    conn
      .query_row(
        "SELECT picture FROM users WHERE username = ?1",
        [username],
        |row| row.get::<_, Option<Vec<u8>>>(0),
      )
      .optional()?
      .flatten(),
  )
}

/// Deletes `username`'s row and block; false when there was no row.
pub fn delete(conn: &mut Connection, username: &str) -> rusqlite::Result<bool> {
  let tx = conn.transaction()?;
  tx.execute("DELETE FROM blocks WHERE username = ?1", [username])?;
  let deleted = tx.execute("DELETE FROM users WHERE username = ?1", [username])?;
  tx.commit()?;
  Ok(deleted == 1)
}
