//! The SQLite databases: opening one with its migrations applied, and running reads and writes
//! on it without blocking the runtime.
//!
//! Each database has one writer connection behind an async mutex and a small pool of read-only
//! connections. Every call runs on a blocking thread. With `flags.volatile` a database lives in
//! memory, nothing reaches the disk, and reads go through the writer.

mod migrate;

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, NaiveTime, SecondsFormat, Utc};
use rusqlite::trace::{TraceEvent, TraceEventCodes};
use rusqlite::{Connection, OpenFlags};
use tokio::sync::Semaphore;
use tracing::{debug, error, info, trace, warn};

use crate::lock;

pub use crate::schema;
pub use migrate::{Migration, Schema};

/// How long a connection waits for a lock another one holds.
const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

/// What went wrong with a database.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
  /// SQLite refused a statement or could not open the file
  #[error(transparent)]
  Sqlite(#[from] rusqlite::Error),
  /// the database file could not be created
  #[error("{0} could not be created: {1}")]
  Create(PathBuf, std::io::Error),
  /// the blocking thread running a call panicked or was canceled
  #[error("a database call did not finish: {0}")]
  Task(String),
  /// the file holds tables but no migration record, so it is not one of reseek's
  #[error("{0} is not a reseek database: it has tables but no schema_migrations table")]
  NotOurs(PathBuf),
  /// a migration recorded in the file differs from the one this program carries, so the file
  /// cannot be brought up to date
  #[error(
    "{} was made with a migration {version} ({name}) of {database} that differs from this \
     reseek's, so it cannot be brought up to date; move the data directory that holds it aside \
     and start reseek again to make a new one",
    path.display()
  )]
  Checksum {
    /// the file
    path: PathBuf,
    /// the database
    database: &'static str,
    /// the migration's version
    version: u32,
    /// the migration's name
    name: String,
  },
  /// the database was migrated by a newer program
  #[error("{database} has migration {version}, which this version of reseek does not know")]
  Newer {
    /// the database
    database: &'static str,
    /// the unknown version
    version: u32,
  },
}

/// Where and how the databases are opened.
#[derive(Clone, Debug)]
pub struct Location {
  /// the directory the database files are in
  pub data: PathBuf,
  /// the directory backups are written to before migrations run
  pub backups: PathBuf,
  /// keep the databases in memory
  pub volatile: bool,
  /// log every statement with its values at info
  pub log_sql: bool,
}

#[cfg(any(test, feature = "testing"))]
impl Location {
  /// Databases kept in memory, for tests.
  pub fn memory() -> Self {
    Self {
      data: std::env::temp_dir(),
      backups: std::env::temp_dir(),
      volatile: true,
      log_sql: false,
    }
  }

  /// Databases in files under `dir`, in the `data` and `backups` directories it makes there,
  /// for tests that reopen a database or migrate one from an older schema.
  pub fn on_disk(dir: &std::path::Path) -> Self {
    let location = Self {
      data: dir.join("data"),
      backups: dir.join("backups"),
      volatile: false,
      log_sql: false,
    };
    std::fs::create_dir_all(&location.data).unwrap();
    std::fs::create_dir_all(&location.backups).unwrap();
    location
  }
}

/// One SQLite database. Cloning gives another handle to the same connections.
#[derive(Clone)]
pub struct Database {
  inner: Arc<Inner>,
}

struct Inner {
  name: &'static str,
  /// the file, none for a database in memory
  path: Option<PathBuf>,
  writer: Arc<tokio::sync::Mutex<Connection>>,
  /// none for a database in memory, whose reads go through the writer
  readers: Option<Readers>,
}

/// The read-only connections not in use, with a permit for each.
struct Readers {
  idle: Arc<Mutex<Vec<Connection>>>,
  available: Arc<Semaphore>,
  source: Arc<Source>,
}

impl Readers {
  /// Opens the pool's connections from `source`.
  fn open(source: Source) -> Result<Self, DbError> {
    let idle = (0..reader_count())
      .map(|_| source.open())
      .collect::<Result<Vec<_>, DbError>>()?;
    Ok(Self {
      available: Arc::new(Semaphore::new(idle.len())),
      idle: Arc::new(Mutex::new(idle)),
      source: Arc::new(source),
    })
  }
}

/// How a database's read-only connections are opened.
struct Source {
  /// the file, or a `file:` URI when `flags` has `SQLITE_OPEN_URI`
  target: PathBuf,
  flags: OpenFlags,
  log_sql: bool,
}

impl Source {
  fn open(&self) -> Result<Connection, DbError> {
    let mut conn = Connection::open_with_flags(&self.target, self.flags)?;
    configure(&mut conn, self.log_sql)?;
    Ok(conn)
  }
}

impl Database {
  /// Opens the database `schema` describes in `location`, creating it when it is new, and
  /// brings it to the newest migration, backing it up first when it already had data.
  pub async fn open(schema: &'static Schema, location: &Location) -> Result<Self, DbError> {
    Self::open_as(schema, location, schema.name).await
  }

  /// An empty database of `schema` in memory, for tests.
  #[cfg(any(test, feature = "testing"))]
  pub async fn memory(schema: &'static Schema) -> Self {
    Self::open(schema, &Location::memory()).await.unwrap()
  }

  /// Opens the database `schema` describes as [`Database::open`] does, in the file
  /// `<stem>.db`, for a schema whose databases are several files.
  pub async fn open_as(
    schema: &'static Schema,
    location: &Location,
    stem: &str,
  ) -> Result<Self, DbError> {
    let location = location.clone();
    let file = format!("{stem}.db");
    let opened = tokio::task::spawn_blocking(move || open_blocking(schema, &location, &file))
      .await
      .map_err(|e| DbError::Task(e.to_string()))??;
    Ok(opened)
  }

  /// Opens the finished database file at `path`, which nothing writes while it is open, for
  /// reading only: without locks or journal files, so the file can be replaced under it and
  /// the connections already open read the file they opened. Writes through it fail.
  pub async fn open_immutable(
    schema: &'static Schema,
    path: &std::path::Path,
  ) -> Result<Self, DbError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
      let mut uri = url::Url::from_file_path(&path)
        .map_err(|()| DbError::Create(path.clone(), std::io::ErrorKind::InvalidInput.into()))?;
      uri.set_query(Some("immutable=1"));
      let source = Source {
        target: PathBuf::from(uri.as_str()),
        flags: OpenFlags::SQLITE_OPEN_READ_ONLY
          | OpenFlags::SQLITE_OPEN_NO_MUTEX
          | OpenFlags::SQLITE_OPEN_URI,
        log_sql: false,
      };
      let writer = source.open()?;
      let readers = Readers::open(source)?;
      debug!(database = schema.name, path = %path.display(), "database opened for reading only");
      Ok(Database {
        inner: Arc::new(Inner {
          name: schema.name,
          path: Some(path),
          writer: Arc::new(tokio::sync::Mutex::new(writer)),
          readers: Some(readers),
        }),
      })
    })
    .await
    .map_err(|e| DbError::Task(e.to_string()))?
  }

  /// The database's name, the stem of its schema.
  pub fn name(&self) -> &'static str {
    self.inner.name
  }

  /// The database's file, `None` for one in memory.
  pub fn path(&self) -> Option<&std::path::Path> {
    self.inner.path.as_deref()
  }

  /// Rebuilds the database file without its free pages, then empties the write-ahead log into
  /// it.
  pub async fn vacuum(&self) -> Result<(), DbError> {
    self
      .write(|conn| conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);"))
      .await
  }

  /// Runs `f` on a read-only connection.
  pub async fn read<T, F>(&self, f: F) -> Result<T, DbError>
  where
    T: Send + 'static,
    F: FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
  {
    let Some(readers) = &self.inner.readers else {
      return self.write(|conn| f(conn)).await;
    };
    let permit = readers
      .available
      .clone()
      .acquire_owned()
      .await
      .map_err(|e| DbError::Task(e.to_string()))?;
    let idle = readers.idle.clone();
    let source = readers.source.clone();
    let name = self.inner.name;
    let conn = lock(&idle)
      .pop()
      .ok_or_else(|| DbError::Task("no idle reader although a permit was free".to_string()))?;
    // the permit is released only once a connection is back in the pool. a call that panicked
    // may have left its connection inside a transaction, so a fresh one takes its place
    let outcome = tokio::task::spawn_blocking(move || {
      match std::panic::catch_unwind(AssertUnwindSafe(|| f(&conn))) {
        Ok(result) => {
          lock(&idle).push(conn);
          drop(permit);
          Ok(result)
        }
        Err(panic) => {
          drop(conn);
          match source.open() {
            Ok(fresh) => {
              lock(&idle).push(fresh);
              drop(permit);
            }
            Err(e) => {
              error!(
                database = name,
                error = %e,
                "a read connection could not be reopened, so the database has one reader fewer"
              );
              permit.forget();
            }
          }
          Err(panic_message(&*panic))
        }
      }
    })
    .await;
    match outcome {
      Ok(Ok(result)) => Ok(result?),
      Ok(Err(message)) => {
        warn!(database = name, panic = message, "a database read panicked");
        Err(DbError::Task(message))
      }
      Err(e) => {
        warn!(database = name, error = %e, "a database read did not finish");
        Err(DbError::Task(e.to_string()))
      }
    }
  }

  /// Runs `f` on the writer connection, one call at a time.
  pub async fn write<T, F>(&self, f: F) -> Result<T, DbError>
  where
    T: Send + 'static,
    F: FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
  {
    let mut conn = self.inner.writer.clone().lock_owned().await;
    let result = tokio::task::spawn_blocking(move || f(&mut conn)).await;
    match result {
      Ok(result) => Ok(result?),
      Err(e) => {
        warn!(database = self.inner.name, error = %e, "a database write did not finish");
        Err(DbError::Task(e.to_string()))
      }
    }
  }

  /// Lets SQLite update its query statistics before the database is closed.
  pub async fn close(&self) {
    let name = self.inner.name;
    if let Err(e) = self
      .write(|conn| conn.execute_batch("PRAGMA optimize;"))
      .await
    {
      warn!(database = name, error = %e, "database not optimized on close");
    }
    debug!(database = name, "database closed");
  }
}

/// Removes the database file at `path` and the `-wal` and `-shm` files SQLite keeps beside it,
/// each when it is there; one that is there and stays is logged. It blocks, so callers run it
/// on a blocking thread.
pub fn remove_database(path: &std::path::Path) {
  for suffix in ["", "-wal", "-shm"] {
    let mut file = path.as_os_str().to_owned();
    file.push(suffix);
    match std::fs::remove_file(&file) {
      Ok(()) => {}
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
      Err(e) => {
        warn!(file = %std::path::Path::new(&file).display(), error = %e, "database file not removed")
      }
    }
  }
}

/// The message a panic carried.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
  panic
    .downcast_ref::<&str>()
    .map(|s| s.to_string())
    .or_else(|| panic.downcast_ref::<String>().cloned())
    .unwrap_or_else(|| "a call panicked without a message".to_string())
}

/// How many read-only connections a database keeps: half the processors, at least two.
fn reader_count() -> usize {
  let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
  (processors / 2).max(2)
}

fn open_blocking(
  schema: &'static Schema,
  location: &Location,
  file: &str,
) -> Result<Database, DbError> {
  let path = location.data.join(file);
  // a name of its own per opening, so two databases in one process never share memory
  let memory = format!("file:{}-{}?mode=memory", schema.name, uuid::Uuid::new_v4());
  let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
    | OpenFlags::SQLITE_OPEN_CREATE
    | OpenFlags::SQLITE_OPEN_NO_MUTEX
    | OpenFlags::SQLITE_OPEN_URI;
  let mut writer = if location.volatile {
    Connection::open_with_flags(&memory, flags)?
  } else {
    if schema.private {
      create_private(&path)?;
    }
    Connection::open_with_flags(&path, flags)?
  };
  configure(&mut writer, location.log_sql)?;
  if !location.volatile {
    writer.pragma_update(None, "journal_mode", "WAL")?;
    check_pragmas(schema.name, &writer)?;
  }
  let backup = (!location.volatile).then_some(location.backups.as_path());
  migrate::run(schema, &mut writer, &path, backup)?;
  writer.execute_batch("PRAGMA optimize;")?;
  let readers = if location.volatile {
    None
  } else {
    Some(Readers::open(Source {
      target: path.clone(),
      flags: OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
      log_sql: location.log_sql,
    })?)
  };
  let place = if location.volatile {
    "memory".to_string()
  } else {
    path.display().to_string()
  };
  let path = (!location.volatile).then_some(path);
  info!(
    database = schema.name,
    path = place,
    readers = readers
      .as_ref()
      .map_or(0, |r| r.available.available_permits()),
    "database opened"
  );
  Ok(Database {
    inner: Arc::new(Inner {
      name: schema.name,
      path,
      writer: Arc::new(tokio::sync::Mutex::new(writer)),
      readers,
    }),
  })
}

/// Creates the empty file at `path` readable and writable by its owner only, unless it exists.
/// SQLite gives the database's `-wal` and `-shm` files the database file's mode.
fn create_private(path: &std::path::Path) -> Result<(), DbError> {
  let mut options = std::fs::OpenOptions::new();
  options.write(true).create_new(true);
  #[cfg(unix)]
  std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
  match options.open(path) {
    Ok(_) => {
      debug!(path = %path.display(), "database file created for its owner only");
      Ok(())
    }
    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
    Err(e) => Err(DbError::Create(path.to_path_buf(), e)),
  }
}

/// The settings every connection takes when it opens.
fn configure(conn: &mut Connection, log_sql: bool) -> rusqlite::Result<()> {
  conn.busy_timeout(BUSY_TIMEOUT)?;
  conn.execute_batch(
    "PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = OFF; PRAGMA temp_store = MEMORY;",
  )?;
  let log: fn(TraceEvent<'_>) = if log_sql {
    statement_with_values
  } else {
    statement
  };
  conn.trace_v2(TraceEventCodes::SQLITE_TRACE_PROFILE, Some(log))
}

/// Warns when the journal mode or the synchronous setting did not take.
fn check_pragmas(name: &str, conn: &Connection) -> rusqlite::Result<()> {
  let mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
  if !mode.eq_ignore_ascii_case("wal") {
    warn!(
      database = name,
      mode, "Failed to set database journal_mode PRAGMA to WAL; performance may be reduced"
    );
  }
  let synchronous: i64 = conn.pragma_query_value(None, "synchronous", |row| row.get(0))?;
  if synchronous != 1 {
    warn!(
      database = name,
      synchronous, "Failed to set database synchronous PRAGMA to NORMAL"
    );
  }
  Ok(())
}

/// Logs a finished statement without its values, which may hold secrets.
fn statement(event: TraceEvent<'_>) {
  if let TraceEvent::Profile(statement, took) = event {
    trace!(sql = %statement.sql(), ms = took.as_secs_f64() * 1000.0, "SQL statement");
  }
}

/// Logs a finished statement with its values, for `flags.log_sql`.
fn statement_with_values(event: TraceEvent<'_>) {
  if let TraceEvent::Profile(statement, took) = event {
    let sql = statement
      .expanded_sql()
      .unwrap_or_else(|| statement.sql().into_owned());
    info!(sql, ms = took.as_secs_f64() * 1000.0, "SQL statement");
  }
}

/// A time as the databases store it: UTC RFC 3339 with microseconds, so text order is time
/// order.
pub fn timestamp(time: DateTime<Utc>) -> String {
  time.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// The earliest time the databases store: the start of the year 0. Stored text sorts in time
/// order only between it and [`latest`].
pub fn earliest() -> DateTime<Utc> {
  NaiveDate::from_ymd_opt(0, 1, 1)
    .expect("year 0 is a date")
    .and_time(NaiveTime::MIN)
    .and_utc()
}

/// The latest time the databases store: the end of the year 9999.
pub fn latest() -> DateTime<Utc> {
  NaiveDate::from_ymd_opt(9999, 12, 31)
    .expect("year 9999 is a date")
    .and_hms_opt(23, 59, 59)
    .expect("a time of day")
    .and_utc()
}

/// `start` moved by `by`, held between [`earliest`] and [`latest`]; a duration too long for
/// the calendar ends at the bound.
pub fn shifted(start: DateTime<Utc>, by: chrono::Duration) -> DateTime<Utc> {
  let bound = if by < chrono::Duration::zero() {
    earliest()
  } else {
    latest()
  };
  start
    .checked_add_signed(by)
    .unwrap_or(bound)
    .clamp(earliest(), latest())
}

/// A value of an enum of plain variants as serde spells it, which is how the databases store
/// one.
pub fn enum_text<T: serde::Serialize>(value: T) -> String {
  match serde_json::to_value(value) {
    Ok(serde_json::Value::String(text)) => text,
    other => unreachable!("an enum of plain variants is written as a string, not {other:?}"),
  }
}

/// The enum stored with [`enum_text`] in the column `at` of `row`. A spelling no variant has is
/// an error that names it.
pub fn enum_at<T: serde::de::DeserializeOwned>(
  row: &rusqlite::Row,
  at: usize,
) -> rusqlite::Result<T> {
  let text: String = row.get(at)?;
  serde_json::from_value(serde_json::Value::String(text)).map_err(|e| {
    rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, e.into())
  })
}

/// A time the databases stored, read back.
pub fn read_timestamp(text: &str) -> rusqlite::Result<DateTime<Utc>> {
  DateTime::parse_from_rfc3339(text)
    .map(|t| t.with_timezone(&Utc))
    .map_err(|e| {
      rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(unix)]
  #[tokio::test]
  async fn a_private_database_and_its_journal_are_for_the_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let location = Location {
      data: dir.path().to_path_buf(),
      backups: dir.path().to_path_buf(),
      volatile: false,
      log_sql: false,
    };
    let db = Database::open(&migrate::tests::PRIVATE, &location)
      .await
      .unwrap();
    db.write(|conn| {
      conn.execute_batch("CREATE TABLE probe (x INTEGER); INSERT INTO probe VALUES (1);")
    })
    .await
    .unwrap();
    for file in ["auth.db", "auth.db-wal", "auth.db-shm"] {
      let mode = std::fs::metadata(dir.path().join(file))
        .unwrap()
        .permissions()
        .mode();
      assert_eq!(mode & 0o777, 0o600, "{file}");
    }
  }

  #[tokio::test]
  async fn a_read_that_panics_is_given_a_fresh_connection() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&migrate::tests::PRIVATE, &Location::on_disk(dir.path()))
      .await
      .unwrap();
    for _ in 0..reader_count() {
      let failed = db
        .read(|_| -> rusqlite::Result<()> { panic!("a read that panics") })
        .await;
      assert!(matches!(failed, Err(DbError::Task(_))), "{failed:?}");
    }
    let reads = async {
      for _ in 0..=reader_count() {
        let one: i64 = db
          .read(|conn| conn.query_row("SELECT 1", [], |row| row.get(0)))
          .await
          .unwrap();
        assert_eq!(one, 1);
      }
    };
    tokio::time::timeout(Duration::from_secs(10), reads)
      .await
      .expect("a read waited for a reader that is gone");
    let readers = db.inner.readers.as_ref().unwrap();
    assert_eq!(readers.available.available_permits(), reader_count());
    assert_eq!(lock(&readers.idle).len(), reader_count());
  }

  #[test]
  fn shifted_times_stay_within_the_years_stored_text_sorts() {
    let now: DateTime<Utc> = "2026-10-25T12:00:00Z".parse().unwrap();
    assert_eq!(
      shifted(now, chrono::Duration::hours(1)),
      "2026-10-25T13:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
    let forever = shifted(now, chrono::Duration::MAX);
    assert_eq!(timestamp(forever), "9999-12-31T23:59:59.000000Z");
    assert!(timestamp(forever) > timestamp(now));
    let long_ago = shifted(now, -chrono::Duration::MAX);
    assert_eq!(timestamp(long_ago), "0000-01-01T00:00:00.000000Z");
    assert!(timestamp(long_ago) < timestamp(now));
    let far = shifted(now, chrono::Duration::days(365 * 100_000));
    assert_eq!(
      far, forever,
      "past the calendar's text form, not past chrono's range"
    );
  }
}
