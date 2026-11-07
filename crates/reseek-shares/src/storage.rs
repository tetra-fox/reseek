//! Where the share index lives: in memory, kept between starts only by its backup, or in
//! `shares.local.db` in the cache directory. Either way a backup is written after every
//! completed scan, to a file beside the old one that then replaces it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use tracing::{info, warn};

use super::repository::SCHEMA;
use crate::db::{Database, DbError, Location, remove_database};
use crate::options::StorageMode;

/// The local host's share index and where its backup is.
pub struct Storage {
  pub db: Database,
  /// the backup file, `None` when nothing is kept between starts
  pub backup: Option<PathBuf>,
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
      let (file, from) = (primary.clone(), backup.clone());
      on_blocking_thread(move || replace_with_backup(&file, from.as_deref())).await?;
      match Database::open_as(&SCHEMA, &location(false), STEM).await {
        Ok(db) => db,
        Err(e) => {
          warn!(error = %e, "Share cache and backup are both missing, corrupt, or out of date");
          let file = primary.clone();
          on_blocking_thread(move || remove_database(&file)).await?;
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

/// Runs file work that may block, such as on a slow disk, off the runtime.
async fn on_blocking_thread(f: impl FnOnce() + Send + 'static) -> Result<(), DbError> {
  tokio::task::spawn_blocking(f)
    .await
    .map_err(|e| DbError::Task(e.to_string()))
}

/// Puts a copy of `backup`, when it is a valid index, in place of the index file `primary`,
/// which is removed either way.
fn replace_with_backup(primary: &Path, backup: Option<&Path>) {
  remove_database(primary);
  let Some(backup) = backup else {
    return;
  };
  match valid(backup) {
    Ok(()) => {
      if let Err(e) = std::fs::copy(backup, primary) {
        warn!(error = %e, "share cache backup not copied");
        remove_database(primary);
      }
    }
    Err(e) => warn!(error = e, "share cache backup not used"),
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

/// The tables, indexes and triggers of a database with the SQL that made each, by name.
type Objects = Vec<(String, String, Option<String>)>;

/// What `conn` holds besides SQLite's own objects.
fn objects(conn: &Connection) -> rusqlite::Result<Objects> {
  let mut statement = conn.prepare(
    "SELECT type, name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
  )?;
  statement
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
    .collect()
}

/// The objects of an index this program writes, read once from one made in memory.
async fn expected_objects() -> Result<&'static Objects, DbError> {
  static EXPECTED: tokio::sync::OnceCell<Objects> = tokio::sync::OnceCell::const_new();
  EXPECTED
    .get_or_try_init(|| async {
      let scratch = std::env::temp_dir();
      let location = Location {
        data: scratch.clone(),
        backups: scratch,
        volatile: true,
        log_sql: false,
      };
      let db = Database::open_as(&SCHEMA, &location, "shares.expected").await?;
      db.read(objects).await
    })
    .await
}

/// Whether the file at `path`, another instance's index, has exactly the migrations and the
/// tables, indexes and triggers this program writes, and every page of it reads as SQLite's
/// quick check expects. Says what is wrong when it does not.
pub async fn check_foreign(path: &Path) -> Result<(), String> {
  let expected = expected_objects().await.map_err(|e| e.to_string())?;
  let path = path.to_path_buf();
  tokio::task::spawn_blocking(move || {
    valid(&path)?;
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
      .map_err(|e| e.to_string())?;
    let found = objects(&conn).map_err(|e| e.to_string())?;
    let mut problems = Vec::new();
    for (kind, name, sql) in expected {
      match found.iter().find(|(_, n, _)| n == name) {
        None => problems.push(format!("{kind} {name} is missing")),
        Some((_, _, found)) if found != sql => problems.push(format!("{kind} {name} differs")),
        Some(_) => {}
      }
    }
    for (kind, name, _) in &found {
      if !expected.iter().any(|(_, n, _)| n == name) {
        problems.push(format!("{kind} {name} is unexpected"));
      }
    }
    if !problems.is_empty() {
      return Err(problems.join(", "));
    }
    // the first line of the check is "ok" or its first finding
    let checked = conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0));
    match checked {
      Ok(line) if line == "ok" => Ok(()),
      Ok(finding) => Err(format!("it is corrupt: {finding}")),
      Err(e) => Err(format!("it could not be checked: {e}")),
    }
  })
  .await
  .map_err(|e| e.to_string())?
}

/// The version of the index's schema: the last migration this program writes.
pub fn schema_version() -> u32 {
  SCHEMA
    .migrations
    .last()
    .map(|m| m.version())
    .expect("the share index has migrations")
}

/// Removes a file at `path`, since `VACUUM INTO` refuses a file that is there already.
fn clear_for_vacuum(path: &Path) -> rusqlite::Result<()> {
  match std::fs::remove_file(path) {
    Ok(()) => Ok(()),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
    Err(e) => Err(rusqlite::Error::ToSqlConversionFailure(e.into())),
  }
}

/// Writes a consistent copy of `db` to `path`, in place of a file left there, and returns its
/// size.
pub async fn dump(db: &Database, path: &Path) -> Result<u64, DbError> {
  let target = path.to_path_buf();
  db.write(move |conn| {
    clear_for_vacuum(&target)?;
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy()])?;
    std::fs::metadata(&target)
      .map(|m| m.len())
      .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
  })
  .await
}

/// Writes a consistent copy of `db` to `backup`, through a file beside it that replaces it
/// whole, so a failure leaves the last good backup.
pub async fn back_up(db: &Database, backup: &Path) -> Result<(), DbError> {
  info!("Backing up shared file cache database...");
  let temporary = backup.with_extension("db.tmp");
  let target = backup.to_path_buf();
  db.write(move |conn| {
    clear_for_vacuum(&temporary)?;
    conn.execute("VACUUM INTO ?1", [temporary.to_string_lossy()])?;
    std::fs::rename(&temporary, &target)
      .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
  })
  .await?;
  info!("Shared file cache database backup complete");
  Ok(())
}

/// Takes the checked index `from` off the write-ahead log, so it can be read with no journal
/// files beside it, and renames it to the new host index file `to`, in the same directory.
pub fn move_host_file(from: &Path, to: &Path) -> Result<(), String> {
  Connection::open(from)
    .and_then(|conn| conn.pragma_update(None, "journal_mode", "DELETE"))
    .map_err(|e| format!("{}: {e}", from.display()))?;
  std::fs::rename(from, to).map_err(|e| format!("{}: {e}", to.display()))
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
  async fn a_dump_replaces_a_copy_left_at_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let storage = open(StorageMode::Memory, dir.path(), dir.path(), false)
      .await
      .unwrap();
    let uploads = dir.path().join("uploads");
    std::fs::create_dir_all(&uploads).unwrap();
    let copy = uploads.join("share_index.db");
    std::fs::write(&copy, b"what an interrupted upload left").unwrap();
    let bytes = dump(&storage.db, &copy).await.unwrap();
    assert_eq!(valid(&copy), Ok(()));
    assert_eq!(std::fs::metadata(&copy).unwrap().len(), bytes);
    assert_eq!(
      std::fs::read_dir(&uploads).unwrap().count(),
      1,
      "replaced, not added beside"
    );
  }

  #[tokio::test]
  async fn a_foreign_index_with_a_corrupt_page_is_refused() {
    use std::io::{Seek, SeekFrom, Write};
    let dir = tempfile::tempdir().unwrap();
    let storage = open(StorageMode::Memory, dir.path(), dir.path(), false)
      .await
      .unwrap();
    let upload = dir.path().join("upload.db");
    dump(&storage.db, &upload).await.unwrap();
    assert_eq!(check_foreign(&upload).await, Ok(()));
    // a table the schema checks do not read, so only the quick check can find the damage
    let (page, page_size): (u32, u32) = {
      let conn = Connection::open(&upload).unwrap();
      let page = conn
        .query_row(
          "SELECT rootpage FROM sqlite_master WHERE type = 'table' AND rootpage > 1 \
           AND name != 'schema_migrations' ORDER BY rootpage DESC LIMIT 1",
          [],
          |row| row.get(0),
        )
        .unwrap();
      let size = conn
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .unwrap();
      (page, size)
    };
    let mut file = std::fs::OpenOptions::new()
      .write(true)
      .open(&upload)
      .unwrap();
    file
      .seek(SeekFrom::Start(u64::from(page - 1) * u64::from(page_size)))
      .unwrap();
    file.write_all(&vec![0xff; page_size as usize]).unwrap();
    drop(file);
    let refused = check_foreign(&upload).await.unwrap_err();
    assert!(
      refused.starts_with("it is corrupt") || refused.starts_with("it could not be checked"),
      "refused by the quick check: {refused}"
    );
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
