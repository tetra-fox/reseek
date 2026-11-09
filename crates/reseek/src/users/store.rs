//! `users.db`: the row kept for every user we dealt with, read and written.

use api_types::state::UserStatistics;
use api_types::users::{Buddy, UserPresence};
use chrono::{DateTime, Utc};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::{self, Schema, read_timestamp, timestamp};

/// `users.db`: the `users`, `blocks` and `memberships` tables.
pub static SCHEMA: Schema = db::schema!(
  "users",
  ["0001_initial", "0002_drop_interests", "0003_groups"]
);

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
  /// the tag of the picture we keep of it, none when we keep none
  pub picture_tag: Option<String>,
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
  /// the presence last observed, with when it was first observed after another one
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
  /// the groups we put them in, by name
  pub groups: Vec<String>,
  /// their notifications are off
  pub muted: bool,
  /// they are our buddy, and how
  pub buddy: Option<Buddy>,
}

/// The columns [`stored`] reads from `users`, in its order; the last is the user's groups.
pub const COLUMNS: &str = "username, first_seen_at, last_seen_at, last_presence, presence_at, \
                           is_privileged, average_speed, upload_count, file_count, \
                           directory_count, statistics_at, country_code, description, \
                           has_picture, upload_slots, queue_length, has_free_slot, info_at, \
                           notes, muted, buddy, picture_tag, notify_presence, \
                           (SELECT json_group_array(group_name) FROM memberships m \
                           WHERE m.username = users.username)";

/// How many columns [`COLUMNS`] names.
pub const COLUMN_COUNT: usize = 24;

/// Every membership, as the user and the group.
pub fn memberships(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
  conn
    .prepare("SELECT username, group_name FROM memberships ORDER BY username, group_name")?
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
    .collect()
}

/// Every buddy, by name.
pub fn buddies(conn: &Connection) -> rusqlite::Result<Vec<(String, Buddy)>> {
  conn
    .prepare("SELECT username, notify_presence FROM users WHERE buddy = 1")?
    .query_map([], |row| {
      Ok((
        row.get(0)?,
        Buddy {
          notify_presence: row.get(1)?,
        },
      ))
    })?
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
      picture_tag: row.get(21)?,
    }),
    None => None,
  };
  let groups: String = row.get(23)?;
  let mut groups: Vec<String> =
    serde_json::from_str(&groups).map_err(|e| conversion(23, e.to_string()))?;
  groups.sort();
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
    groups,
    muted: row.get(19)?,
    buddy: row
      .get::<_, bool>(20)?
      .then(|| {
        Ok::<_, rusqlite::Error>(Buddy {
          notify_presence: row.get(22)?,
        })
      })
      .transpose()?,
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

/// Makes sure `username` has a row, seen at `now` unless they were seen later. Writes do not
/// come in the order of what they record, as touches wait up to a second and info does not.
pub fn ensure(conn: &Connection, username: &str, now: DateTime<Utc>) -> rusqlite::Result<()> {
  let now = timestamp(now);
  conn.execute(
    "INSERT INTO users (username, first_seen_at, last_seen_at) VALUES (?1, ?2, ?2) \
     ON CONFLICT (username) DO UPDATE SET \
     last_seen_at = max(last_seen_at, excluded.last_seen_at)",
    params![username, now],
  )?;
  Ok(())
}

/// What a write through the API sets on a user's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
  /// our notes
  Notes(String),
  /// the groups we put them in, each once
  Groups(Vec<String>),
  /// our buddy, and how, or not
  Buddy(Option<Buddy>),
  /// notifications off or on
  Muted(bool),
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
    Change::Groups(groups) => {
      tx.execute("DELETE FROM memberships WHERE username = ?1", [username])?;
      let mut insert =
        tx.prepare("INSERT OR IGNORE INTO memberships (username, group_name) VALUES (?1, ?2)")?;
      for group in groups {
        insert.execute(params![username, group])?;
      }
      groups.len()
    }
    Change::Buddy(buddy) => tx.execute(
      "UPDATE users SET buddy = ?2, notify_presence = ?3 WHERE username = ?1",
      params![
        username,
        buddy.is_some(),
        buddy.is_some_and(|b| b.notify_presence)
      ],
    )?,
    Change::Muted(muted) => tx.execute(
      "UPDATE users SET muted = ?2 WHERE username = ?1",
      params![username, muted],
    )?,
  };
  tx.commit()
}

/// Moves every member of the group `from` into the group `to`, a member of both kept once, and
/// returns who moved.
pub fn move_members(conn: &mut Connection, from: &str, to: &str) -> rusqlite::Result<Vec<String>> {
  let tx = conn.transaction()?;
  let moved = members(&tx, from)?;
  tx.execute(
    "INSERT OR IGNORE INTO memberships (username, group_name) \
     SELECT username, ?2 FROM memberships WHERE group_name = ?1",
    params![from, to],
  )?;
  tx.execute("DELETE FROM memberships WHERE group_name = ?1", [from])?;
  tx.commit()?;
  Ok(moved)
}

/// Takes every member out of the group `group` and returns who they were.
pub fn let_go(conn: &mut Connection, group: &str) -> rusqlite::Result<Vec<String>> {
  let tx = conn.transaction()?;
  let members = members(&tx, group)?;
  tx.execute("DELETE FROM memberships WHERE group_name = ?1", [group])?;
  tx.commit()?;
  Ok(members)
}

fn members(conn: &Connection, group: &str) -> rusqlite::Result<Vec<String>> {
  conn
    .prepare("SELECT username FROM memberships WHERE group_name = ?1 ORDER BY username")?
    .query_map([group], |row| row.get(0))?
    .collect()
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
  tx.execute("DELETE FROM memberships WHERE username = ?1", [username])?;
  let deleted = tx.execute("DELETE FROM users WHERE username = ?1", [username])?;
  tx.commit()?;
  Ok(deleted == 1)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::{Database, Location};

  /// `users.db` before groups had a table of their own.
  static BEFORE_GROUPS: Schema = db::schema!("users", ["0001_initial", "0002_drop_interests"]);

  #[tokio::test]
  async fn tags_upload_groups_and_trusted_buddies_become_memberships() {
    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    let old = Database::open(&BEFORE_GROUPS, &location).await.unwrap();
    old
      .write(|conn| {
        for (username, tags, group, buddy, trusted) in [
          (
            "alice",
            r#"["jazz","trusted"]"#,
            Some("Friends"),
            true,
            true,
          ),
          ("bob", "[]", None, true, false),
          ("carol", r#"["jazz"]"#, Some("jazz"), false, false),
        ] {
          conn.execute(
            "INSERT INTO users (username, first_seen_at, last_seen_at, tags, group_override, \
             buddy, trusted) VALUES (?1, '2026-11-01T00:00:00Z', '2026-11-01T00:00:00Z', ?2, \
             ?3, ?4, ?5)",
            params![username, tags, group, buddy, trusted],
          )?;
        }
        Ok(())
      })
      .await
      .unwrap();
    drop(old);
    let migrated = Database::open(&SCHEMA, &location).await.unwrap();
    let (all, alice, bob) = migrated
      .read(|conn| Ok((memberships(conn)?, get(conn, "alice")?, get(conn, "bob")?)))
      .await
      .unwrap();
    let pairs = |list: &[(&str, &str)]| {
      list
        .iter()
        .map(|(u, g)| (u.to_string(), g.to_string()))
        .collect::<Vec<_>>()
    };
    assert_eq!(
      all,
      pairs(&[
        ("alice", "friends"),
        ("alice", "jazz"),
        ("alice", "trusted"),
        ("carol", "jazz"),
      ]),
      "a group named twice is one membership, and an upload group's name is lower case"
    );
    assert_eq!(alice.unwrap().groups, ["friends", "jazz", "trusted"]);
    let bob = bob.unwrap();
    assert!(bob.groups.is_empty());
    assert!(
      bob.buddy.is_some(),
      "a buddy who was not trusted stays a buddy"
    );
  }
}
