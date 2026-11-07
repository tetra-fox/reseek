//! Migrations: numbered SQL files under `migrations/<database>/`, built into the program, each
//! applied once in its own transaction and recorded with a checksum in `schema_migrations`.

use std::path::Path;
use std::time::Instant;

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::{DbError, timestamp};

/// A database's name, which is also its file's stem, and its migrations in order.
#[derive(Debug)]
pub struct Schema {
  /// the name, such as `auth` for `auth.db`
  pub name: &'static str,
  /// the migrations, oldest first
  pub migrations: &'static [Migration],
  /// the file holds credentials, so on Unix it is created readable by its owner only
  pub private: bool,
}

/// One migration file.
#[derive(Debug)]
pub struct Migration {
  /// the file's stem, such as `0001_initial`: the version, an underscore and a name
  pub file: &'static str,
  /// the SQL it runs
  pub sql: &'static str,
}

impl Migration {
  /// The version, the number the file's stem starts with.
  pub fn version(&self) -> u32 {
    self
      .file
      .split_once('_')
      .and_then(|(number, _)| number.parse().ok())
      .unwrap_or_else(|| panic!("migration file {} has no version", self.file))
  }

  /// The name after the version.
  pub fn name(&self) -> &'static str {
    self
      .file
      .split_once('_')
      .map_or(self.file, |(_, name)| name)
  }

  fn checksum(&self) -> String {
    let digest = Sha256::digest(self.sql.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
  }
}

/// A database's [`Schema`], its migration files read from `migrations/<name>/` at build time:
/// `schema!("events", ["0001_initial"])`, or `schema!("auth", ["0001_initial"], private)` for
/// a database only its owner may read.
#[macro_export]
macro_rules! schema {
  ($name:literal, [$($file:literal),* $(,)?]) => {
    $crate::db::schema!(@build $name, [$($file),*], false)
  };
  ($name:literal, [$($file:literal),* $(,)?], private) => {
    $crate::db::schema!(@build $name, [$($file),*], true)
  };
  (@build $name:literal, [$($file:literal),*], $private:literal) => {
    $crate::db::Schema {
      name: $name,
      migrations: &[$($crate::db::Migration {
        file: $file,
        sql: include_str!(concat!(
          env!("CARGO_MANIFEST_DIR"), "/../../migrations/", $name, "/", $file, ".sql"
        )),
      }),*],
      private: $private,
    }
  };
}

const RECORD: &str = "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT \
                      NOT NULL, applied_at TEXT NOT NULL, checksum TEXT NOT NULL)";

/// Brings the database on `conn` to the newest migration of `schema`. A database that has data
/// is copied into `backups` first, when given; `path` names the file in messages and backups.
pub(super) fn run(
  schema: &Schema,
  conn: &mut Connection,
  path: &Path,
  backups: Option<&Path>,
) -> Result<(), DbError> {
  let objects: i64 = conn.query_row(
    "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
    [],
    |row| row.get(0),
  )?;
  let fresh = objects == 0;
  let applied = if fresh {
    conn.execute_batch(RECORD)?;
    Vec::new()
  } else {
    let recorded: Option<String> = conn
      .query_row(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
        [],
        |row| row.get(0),
      )
      .optional()?;
    if recorded.is_none() {
      return Err(DbError::NotOurs(path.to_path_buf()));
    }
    let mut statement =
      conn.prepare("SELECT version, name, checksum FROM schema_migrations ORDER BY version")?;
    statement
      .query_map([], |row| {
        Ok((
          row.get::<_, u32>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, String>(2)?,
        ))
      })?
      .collect::<rusqlite::Result<Vec<_>>>()?
  };
  for (version, name, checksum) in &applied {
    let Some(known) = schema.migrations.iter().find(|m| m.version() == *version) else {
      return Err(DbError::Newer {
        database: schema.name,
        version: *version,
      });
    };
    if known.checksum() != *checksum {
      return Err(DbError::Checksum {
        path: path.to_path_buf(),
        database: schema.name,
        version: *version,
        name: name.clone(),
      });
    }
  }
  let current = applied.last().map_or(0, |(version, ..)| *version);
  let pending: Vec<&Migration> = schema
    .migrations
    .iter()
    .filter(|m| m.version() > current)
    .collect();
  if pending.is_empty() {
    info!(
      database = schema.name,
      version = current,
      "Database is up to date"
    );
    return Ok(());
  }
  if let Some(backups) = backups.filter(|_| !fresh) {
    backup(conn, schema, current, backups)?;
  }
  warn!(
    database = schema.name,
    count = pending.len(),
    "outstanding database migration(s) to apply"
  );
  let total = pending.len();
  for (index, migration) in pending.into_iter().enumerate() {
    let started = Instant::now();
    info!(
      database = schema.name,
      migration = migration.file,
      step = index + 1,
      of = total,
      "Applying migration"
    );
    let tx = conn.transaction()?;
    tx.execute_batch(migration.sql)?;
    tx.execute(
      "INSERT INTO schema_migrations (version, name, applied_at, checksum) VALUES (?1, ?2, ?3, \
       ?4)",
      params![
        migration.version(),
        migration.name(),
        timestamp(Utc::now()),
        migration.checksum()
      ],
    )?;
    tx.commit()?;
    info!(
      database = schema.name,
      migration = migration.file,
      ms = started.elapsed().as_millis(),
      "migration applied"
    );
  }
  Ok(())
}

/// Copies the database as it stands into `<backups>/<name>.db.<version>.bak.db`, a consistent
/// copy even with the write-ahead log. The copy of a private database is private too.
fn backup(conn: &Connection, schema: &Schema, version: u32, backups: &Path) -> Result<(), DbError> {
  let name = schema.name;
  let target = backups.join(format!("{name}.db.{version:04}.bak.db"));
  // VACUUM INTO refuses a file that exists, and an older backup of the same version is stale
  match std::fs::remove_file(&target) {
    Ok(()) => {}
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
    Err(e) => {
      warn!(path = %target.display(), error = %e, "old database backup not removed");
    }
  }
  // VACUUM INTO writes into an empty file that exists and keeps its mode
  if schema.private {
    super::create_private(&target)?;
  }
  conn.execute("VACUUM INTO ?1", [target.to_string_lossy()])?;
  info!(database = name, path = %target.display(), "database backed up before migrating");
  Ok(())
}

#[cfg(test)]
pub(super) mod tests {
  use super::*;
  use crate::db::{Database, Location};

  /// A database only its owner may read, made with the migrations of `auth.db`.
  pub static PRIVATE: Schema = crate::schema!("auth", ["0001_initial"], private);

  use PRIVATE as TEST;

  #[test]
  fn every_migration_file_is_built_in_and_numbered_in_order() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    for schema in [&TEST] {
      let mut on_disk: Vec<String> = std::fs::read_dir(root.join(schema.name))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
      on_disk.sort();
      let built_in: Vec<String> = schema
        .migrations
        .iter()
        .map(|m| format!("{}.sql", m.file))
        .collect();
      assert_eq!(built_in, on_disk, "{}", schema.name);
      for (index, migration) in schema.migrations.iter().enumerate() {
        assert_eq!(
          migration.version() as usize,
          index + 1,
          "{}",
          migration.file
        );
      }
    }
  }

  #[tokio::test]
  async fn a_fresh_database_is_migrated_and_reopens_without_changes() {
    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    let db = Database::open(&TEST, &location).await.unwrap();
    let tables: Vec<String> = db
      .read(|conn| {
        conn
          .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?
          .query_map([], |row| row.get(0))?
          .collect()
      })
      .await
      .unwrap();
    assert_eq!(
      tables,
      [
        "api_tokens",
        "identities",
        "keys",
        "schema_migrations",
        "sessions"
      ]
    );
    let mode: String = db
      .read(|conn| conn.pragma_query_value(None, "journal_mode", |row| row.get(0)))
      .await
      .unwrap();
    assert_eq!(mode, "wal");
    db.close().await;
    drop(db);
    Database::open(&TEST, &location).await.unwrap();
    assert_eq!(
      std::fs::read_dir(&location.backups).unwrap().count(),
      0,
      "nothing was pending, so nothing was backed up"
    );
  }

  #[tokio::test]
  async fn foreign_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    let foreign = Connection::open(location.data.join("auth.db")).unwrap();
    foreign
      .execute_batch("CREATE TABLE Events (Id TEXT)")
      .unwrap();
    drop(foreign);
    assert!(matches!(
      Database::open(&TEST, &location).await,
      Err(DbError::NotOurs(_))
    ));
  }

  #[tokio::test]
  async fn a_database_made_with_other_migrations_is_refused_untouched() {
    static CHAIN: Schema = Schema {
      private: false,
      name: "auth",
      migrations: &[
        Migration {
          file: "0001_initial",
          sql: "CREATE TABLE a (x INTEGER)",
        },
        Migration {
          file: "0002_more",
          sql: "CREATE TABLE b (y INTEGER)",
        },
      ],
    };
    static FOLDED: Schema = Schema {
      private: false,
      name: "auth",
      migrations: &[Migration {
        file: "0001_initial",
        sql: "CREATE TABLE a (x INTEGER); CREATE TABLE b (y INTEGER)",
      }],
    };
    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    let db = Database::open(&CHAIN, &location).await.unwrap();
    db.write(|conn| conn.execute("INSERT INTO a VALUES (7)", []))
      .await
      .unwrap();
    db.close().await;
    drop(db);

    let refused = Database::open(&FOLDED, &location).await.err().unwrap();
    assert!(
      matches!(refused, DbError::Checksum { version: 1, .. }),
      "{refused:?}"
    );
    let message = refused.to_string();
    assert!(
      message.contains(&location.data.join("auth.db").display().to_string())
        && message.contains("data directory"),
      "the message names the file and what to do: {message}"
    );
    let kept = Connection::open(location.data.join("auth.db")).unwrap();
    let versions: Vec<u32> = kept
      .prepare("SELECT version FROM schema_migrations ORDER BY version")
      .unwrap()
      .query_map([], |row| row.get(0))
      .unwrap()
      .collect::<rusqlite::Result<_>>()
      .unwrap();
    assert_eq!(versions, [1, 2], "nothing is recorded on top");
    let x: i64 = kept
      .query_row("SELECT x FROM a", [], |row| row.get(0))
      .unwrap();
    assert_eq!(x, 7);
    assert_eq!(
      std::fs::read_dir(&location.backups).unwrap().count(),
      0,
      "nothing was applied, so nothing was backed up"
    );
  }

  #[tokio::test]
  async fn a_database_with_data_is_backed_up_for_its_owner_before_a_pending_migration() {
    static TWO: Schema = Schema {
      private: true,
      name: "auth",
      migrations: &[
        Migration {
          file: "0001_initial",
          sql: "CREATE TABLE a (x INTEGER)",
        },
        Migration {
          file: "0002_more",
          sql: "CREATE TABLE b (y INTEGER)",
        },
      ],
    };
    static ONE: Schema = Schema {
      private: true,
      name: "auth",
      migrations: &[Migration {
        file: "0001_initial",
        sql: "CREATE TABLE a (x INTEGER)",
      }],
    };
    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    let db = Database::open(&ONE, &location).await.unwrap();
    db.write(|conn| conn.execute("INSERT INTO a VALUES (7)", []))
      .await
      .unwrap();
    drop(db);
    let db = Database::open(&TWO, &location).await.unwrap();
    let versions: Vec<u32> = db
      .read(|conn| {
        conn
          .prepare("SELECT version FROM schema_migrations")?
          .query_map([], |row| row.get(0))?
          .collect()
      })
      .await
      .unwrap();
    assert_eq!(versions, [1, 2]);
    let copy = location.backups.join("auth.db.0001.bak.db");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let mode = std::fs::metadata(&copy).unwrap().permissions().mode();
      assert_eq!(
        mode & 0o777,
        0o600,
        "the copy of a private database is private"
      );
    }
    let backup = Connection::open(&copy).unwrap();
    let kept: i64 = backup
      .query_row("SELECT x FROM a", [], |row| row.get(0))
      .unwrap();
    assert_eq!(kept, 7);
    drop(db);
    assert!(matches!(
      Database::open(&ONE, &location).await,
      Err(DbError::Newer { version: 2, .. })
    ));
  }

  #[tokio::test]
  async fn a_volatile_database_lives_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut location = Location::on_disk(dir.path());
    location.volatile = true;
    let db = Database::open(&TEST, &location).await.unwrap();
    let count: i64 = db
      .read(|conn| conn.query_row("SELECT count(*) FROM sessions", [], |row| row.get(0)))
      .await
      .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
      std::fs::read_dir(&location.data).unwrap().count(),
      0,
      "nothing reaches the disk"
    );
  }
}
