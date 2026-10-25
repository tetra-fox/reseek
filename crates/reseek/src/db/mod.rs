//! The SQLite databases: opening one with its migrations applied, and running reads and writes
//! on it without blocking the runtime.
//!
//! Each database has one writer connection behind an async mutex and a small pool of read-only
//! connections. Every call runs on a blocking thread. With `flags.volatile` a database lives in
//! memory, nothing reaches the disk, and reads go through the writer.

mod migrate;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::trace::{TraceEvent, TraceEventCodes};
use rusqlite::{Connection, OpenFlags};
use tokio::sync::Semaphore;
use tracing::{debug, info, trace, warn};

pub(crate) use migrate::schema;
pub use migrate::{Migration, Schema};

/// How long a connection waits for a lock another one holds.
const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

/// What went wrong with a database.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
  /// SQLite refused a statement or could not open the file
  #[error(transparent)]
  Sqlite(#[from] rusqlite::Error),
  /// the blocking thread running a call panicked or was cancelled
  #[error("a database call did not finish: {0}")]
  Task(String),
  /// the file holds tables but no migration record, so it is not one of reseek's
  #[error("{0} is not a reseek database: it has tables but no schema_migrations table")]
  NotOurs(PathBuf),
  /// a migration this program applied differs from the one it now carries
  #[error("migration {version} ({name}) of {database} changed since it was applied")]
  Checksum {
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

/// One SQLite database. Cloning gives another handle to the same connections.
#[derive(Clone)]
pub struct Database {
  inner: Arc<Inner>,
}

struct Inner {
  name: &'static str,
  writer: Arc<tokio::sync::Mutex<Connection>>,
  /// none for a database in memory, whose reads go through the writer
  readers: Option<Readers>,
}

/// The read-only connections not in use, with a permit for each.
struct Readers {
  idle: Arc<Mutex<Vec<Connection>>>,
  available: Arc<Semaphore>,
}

impl Database {
  /// Opens the database `schema` describes in `location`, creating it when it is new, and
  /// brings it to the newest migration, backing it up first when it already had data.
  pub async fn open(schema: &'static Schema, location: &Location) -> Result<Self, DbError> {
    let location = location.clone();
    let opened = tokio::task::spawn_blocking(move || open_blocking(schema, &location))
      .await
      .map_err(|e| DbError::Task(e.to_string()))??;
    Ok(opened)
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
    let conn = idle
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .pop()
      .ok_or_else(|| DbError::Task("no idle reader although a permit was free".to_string()))?;
    let result = tokio::task::spawn_blocking(move || {
      let result = f(&conn);
      idle
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(conn);
      result
    })
    .await;
    match result {
      Ok(result) => {
        drop(permit);
        Ok(result?)
      }
      Err(e) => {
        // the connection went down with the call, so the pool keeps one permit fewer
        permit.forget();
        warn!(database = self.inner.name, error = %e, "a database read did not finish");
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
      warn!(database = name, error = %e, "database not optimised on close");
    }
    debug!(database = name, "database closed");
  }
}

/// How many read-only connections a database keeps: half the processors, at least two.
fn reader_count() -> usize {
  let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
  (processors / 2).max(2)
}

fn open_blocking(schema: &'static Schema, location: &Location) -> Result<Database, DbError> {
  let file = format!("{}.db", schema.name);
  let path = location.data.join(&file);
  // a name of its own per opening, so two databases in one process never share memory
  let memory = format!("file:{}-{}?mode=memory", schema.name, uuid::Uuid::new_v4());
  let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
    | OpenFlags::SQLITE_OPEN_CREATE
    | OpenFlags::SQLITE_OPEN_NO_MUTEX
    | OpenFlags::SQLITE_OPEN_URI;
  let mut writer = if location.volatile {
    Connection::open_with_flags(&memory, flags)?
  } else {
    Connection::open_with_flags(&path, flags)?
  };
  configure(&writer, location.log_sql)?;
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
    let idle = (0..reader_count())
      .map(|_| {
        let reader = Connection::open_with_flags(
          &path,
          OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure(&reader, location.log_sql)?;
        Ok(reader)
      })
      .collect::<Result<Vec<_>, DbError>>()?;
    Some(Readers {
      available: Arc::new(Semaphore::new(idle.len())),
      idle: Arc::new(Mutex::new(idle)),
    })
  };
  let place = if location.volatile {
    "memory".to_string()
  } else {
    path.display().to_string()
  };
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
      writer: Arc::new(tokio::sync::Mutex::new(writer)),
      readers,
    }),
  })
}

/// The settings every connection takes when it opens.
fn configure(conn: &Connection, log_sql: bool) -> rusqlite::Result<()> {
  conn.busy_timeout(BUSY_TIMEOUT)?;
  conn.execute_batch(
    "PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = OFF; PRAGMA temp_store = MEMORY;",
  )?;
  let log: fn(TraceEvent<'_>) = if log_sql {
    statement_with_values
  } else {
    statement
  };
  conn.trace_v2(TraceEventCodes::SQLITE_TRACE_PROFILE, Some(log));
  Ok(())
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

/// A time the databases stored, read back.
pub fn read_timestamp(text: &str) -> rusqlite::Result<DateTime<Utc>> {
  DateTime::parse_from_rfc3339(text)
    .map(|t| t.with_timezone(&Utc))
    .map_err(|e| {
      rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })
}
