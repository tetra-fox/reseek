//! Where the share index lives: in memory, kept between starts only by its backup, or in
//! `shares.local.db` in the cache directory. Either way a backup is written after every
//! completed scan, to a file beside the old one that then replaces it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use tracing::{info, warn};

use super::repository::SCHEMA;
use crate::config::enums::StorageMode;
use crate::db::{Database, DbError, Location};

/// The local host's share index and where its backup is.
pub struct Storage {
  /// the index
  pub db: Database,
  /// the backup file, `None` when nothing is kept between starts
  pub backup: Option<PathBuf>,
  /// the index is in memory
  pub in_memory: bool,
}

/// The stem of the local host's index file.
const STEM: &str = "shares.local";

/// The local host's backup file in the cache directory `cache`.
pub fn backup_path(cache: &Path) -> PathBuf {
  cache.join(format!("{STEM}.bak.db"))
}

/// Opens the local host's index as `mode` says, in the cache directory `cache`. With
/// `volatile` everything is in memory and nothing is kept. An index file that does not open is
/// replaced by its backup, or else started afresh, which a scan then fills.
pub async fn open(
  mode: StorageMode,
  cache: &Path,
  backups: &Path,
  volatile: bool,
) -> Result<Storage, DbError> {
  let location = |volatile| Location {
    data: cache.to_path_buf(),
    backups: backups.to_path_buf(),
    volatile,
    log_sql: false,
  };
  let backup = (!volatile).then(|| backup_path(cache));
  if volatile || mode == StorageMode::Memory {
    let db = Database::open_as(&SCHEMA, &location(true), STEM).await?;
    return Ok(Storage {
      db,
      backup,
      in_memory: true,
    });
  }
  let primary = cache.join(format!("{STEM}.db"));
  let db = match Database::open_as(&SCHEMA, &location(false), STEM).await {
    Ok(db) => db,
    Err(e) => {
      warn!(
        error = %e,
        "Share cache is missing, corrupt, or is out of date. Attempting to load from backup..."
      );
      remove_database(&primary);
      if let Some(backup) = &backup
        && valid(backup).is_ok()
        && let Err(e) = std::fs::copy(backup, &primary)
      {
        warn!(error = %e, "share cache backup not copied");
        remove_database(&primary);
      }
      match Database::open_as(&SCHEMA, &location(false), STEM).await {
        Ok(db) => db,
        Err(e) => {
          warn!(error = %e, "Share cache and backup are both missing, corrupt, or is out of date");
          remove_database(&primary);
          Database::open_as(&SCHEMA, &location(false), STEM).await?
        }
      }
    }
  };
  Ok(Storage {
    db,
    backup,
    in_memory: false,
  })
}

/// Removes a database file and its journal files, each when it is there.
fn remove_database(path: &Path) {
  for suffix in ["", "-wal", "-shm"] {
    let mut file = path.as_os_str().to_owned();
    file.push(suffix);
    match std::fs::remove_file(&file) {
      Ok(()) => {}
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
      Err(e) => {
        warn!(file = %Path::new(&file).display(), error = %e, "share cache file not removed")
      }
    }
  }
}

/// Whether the file at `path` is an index this program wrote: every migration it knows is
/// recorded and no other. Says what is wrong when it is not.
pub fn valid(path: &Path) -> Result<(), String> {
  let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
    .map_err(|e| e.to_string())?;
  let mut statement = conn
    .prepare("SELECT version FROM schema_migrations ORDER BY version")
    .map_err(|e| e.to_string())?;
  let versions: Vec<u32> = statement
    .query_map([], |row| row.get(0))
    .and_then(Iterator::collect)
    .map_err(|e| e.to_string())?;
  let expected: Vec<u32> = SCHEMA.migrations.iter().map(|m| m.version()).collect();
  if versions != expected {
    return Err(format!(
      "it has the migrations {versions:?}, this program writes {expected:?}"
    ));
  }
  Ok(())
}

/// Writes a consistent copy of `db` to `backup`, through a file beside it that replaces it
/// whole, so a failure leaves the last good backup.
pub async fn back_up(db: &Database, backup: &Path) -> Result<(), DbError> {
  info!("Backing up shared file cache database...");
  let temporary = backup.with_extension("db.tmp");
  let target = backup.to_path_buf();
  db.write(move |conn| {
    match std::fs::remove_file(&temporary) {
      Ok(()) => {}
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
      Err(e) => return Err(rusqlite::Error::ToSqlConversionFailure(e.into())),
    }
    conn.execute("VACUUM INTO ?1", [temporary.to_string_lossy()])?;
    std::fs::rename(&temporary, &target)
      .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
  })
  .await?;
  info!("Shared file cache database backup complete");
  Ok(())
}

/// Replaces what `db` holds with the backup at `backup`.
pub async fn restore(db: &Database, backup: &Path) -> Result<(), DbError> {
  let source = backup.to_path_buf();
  db.write(move |conn| {
    let from = Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let copy = rusqlite::backup::Backup::new(&from, conn)?;
    copy.run_to_completion(1024, Duration::ZERO, None)
  })
  .await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn a_backup_restores_into_a_fresh_index_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let first = open(StorageMode::Memory, dir.path(), dir.path(), false)
      .await
      .unwrap();
    first
      .db
      .write(|conn| super::super::repository::insert_scan(conn, 7, "{}"))
      .await
      .unwrap();
    let backup = first.backup.clone().unwrap();
    back_up(&first.db, &backup).await.unwrap();
    back_up(&first.db, &backup).await.unwrap();
    assert_eq!(valid(&backup), Ok(()));
    let second = open(StorageMode::Memory, dir.path(), dir.path(), false)
      .await
      .unwrap();
    restore(&second.db, &backup).await.unwrap();
    let latest = second
      .db
      .read(super::super::repository::latest_scan)
      .await
      .unwrap();
    assert_eq!(latest.map(|s| s.started_at), Some(7));
    std::fs::write(&backup, b"not a database").unwrap();
    assert!(valid(&backup).is_err());
  }

  #[tokio::test]
  async fn a_disk_index_that_does_not_open_starts_again() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shares.local.db"), b"garbage").unwrap();
    let storage = open(StorageMode::Disk, dir.path(), dir.path(), false)
      .await
      .unwrap();
    assert!(!storage.in_memory);
    assert_eq!(
      storage.db.path(),
      Some(dir.path().join("shares.local.db").as_path())
    );
    let none = storage
      .db
      .read(super::super::repository::latest_scan)
      .await
      .unwrap();
    assert_eq!(none, None);
  }
}
