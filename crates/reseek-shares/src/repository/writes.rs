//! What a scan or a watched change writes into the share index: scan records, directories with
//! their files, pruning, the filename index and the maintenance after a scan.

use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use tracing::{debug, warn};

use super::{DirectoryRows, FileRow, ScanState, upper};

pub fn insert_scan(conn: &Connection, started_at: i64, options: &str) -> rusqlite::Result<()> {
  conn
    .execute(
      "INSERT INTO scans (timestamp, options) VALUES (?1, ?2)",
      params![started_at, options],
    )
    .map(drop)
}

/// Marks the running scan that started at `started_at` finished at `ended_at`.
pub fn end_scan(conn: &Connection, started_at: i64, ended_at: i64) -> rusqlite::Result<()> {
  conn
    .execute(
      "UPDATE scans SET \"end\" = ?1, state = 'finished' WHERE timestamp = ?2 \
       AND state = 'running'",
      params![ended_at, started_at],
    )
    .map(drop)
}

/// Marks the running scan that started at `started_at` stopped at `ended_at` without
/// completing, as `state` says.
pub fn stop_scan(
  conn: &Connection,
  started_at: i64,
  ended_at: i64,
  state: ScanState,
) -> rusqlite::Result<()> {
  conn
    .execute(
      "UPDATE scans SET state = ?1, \"end\" = ?2 WHERE timestamp = ?3 AND state = 'running'",
      params![state.as_str(), ended_at, started_at],
    )
    .map(drop)
}

/// Marks every scan recorded as running failed, at a time not known, and returns how many: run
/// only while no scan runs, when each such row was left by a scan a stop cut off.
pub fn end_interrupted_scans(conn: &Connection) -> rusqlite::Result<usize> {
  conn.execute(
    "UPDATE scans SET state = 'failed' WHERE state = 'running'",
    [],
  )
}

pub fn mark_latest_suspect(conn: &Connection) -> rusqlite::Result<()> {
  conn
    .execute(
      "UPDATE scans SET suspect = 1 WHERE timestamp = \
       (SELECT timestamp FROM scans ORDER BY timestamp DESC LIMIT 1)",
      [],
    )
    .map(drop)
}

/// Writes a directory's row and its files' rows, stamped `timestamp`; a file whose remote name
/// is in the index already is replaced. Returns each file's row id, in order.
pub fn write_directory(
  tx: &Transaction,
  timestamp: i64,
  directory: &DirectoryRows,
) -> rusqlite::Result<Vec<i64>> {
  tx.prepare_cached(
    "INSERT INTO directories (name, timestamp) VALUES (?1, ?2) \
     ON CONFLICT DO UPDATE SET timestamp = excluded.timestamp",
  )?
  .execute(params![directory.name, timestamp])?;
  let mut upsert = tx.prepare_cached(
    "INSERT INTO files (maskedFilename, originalFilename, size, touchedAt, code, extension, \
     attributeJson, timestamp) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7) \
     ON CONFLICT (maskedFilename) DO UPDATE SET originalFilename = excluded.originalFilename, \
     size = excluded.size, touchedAt = excluded.touchedAt, code = excluded.code, \
     extension = excluded.extension, attributeJson = excluded.attributeJson, \
     timestamp = excluded.timestamp RETURNING id",
  )?;
  // a row of the same name already written by this scan is another local file whose name
  // reads the same once masked
  let mut written = tx.prepare_cached(
    "SELECT originalFilename FROM files WHERE maskedFilename = ?1 AND timestamp = ?2",
  )?;
  let mut ids = Vec::with_capacity(directory.files.len());
  for file in &directory.files {
    let earlier: Option<Vec<u8>> = written
      .query_row(params![file.masked, timestamp], |row| row.get(0))
      .optional()?;
    if earlier.is_some_and(|earlier| earlier != file.original) {
      warn!(
        name = file.masked,
        file = %super::bytes_path(file.original.clone()).display(),
        "two local files share one remote name; the later one is shared"
      );
    }
    let json = serde_json::to_string(&file.attributes)
      .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
    let id = upsert.query_row(
      params![
        file.masked,
        file.original,
        i64::try_from(file.size).unwrap_or(i64::MAX),
        file.touched_at,
        file.extension,
        json,
        timestamp
      ],
      |row| row.get(0),
    )?;
    ids.push(id);
  }
  Ok(ids)
}

/// What a scan removed when it finished.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
  /// file rows the scan did not see
  pub files: usize,
  /// directory rows the scan did not see
  pub directories: usize,
}

/// Deletes every row the scan stamped `timestamp` did not see, then rebuilds the filename
/// index from the files left, in one transaction so neither is ever seen half done.
pub fn finish_scan(conn: &mut Connection, timestamp: i64) -> rusqlite::Result<Pruned> {
  let tx = conn.transaction()?;
  let files = tx.execute("DELETE FROM files WHERE timestamp < ?1", [timestamp])?;
  let directories = tx.execute("DELETE FROM directories WHERE timestamp < ?1", [timestamp])?;
  tx.execute_batch(
    "DELETE FROM filenames; \
     INSERT INTO filenames (rowid, maskedFilename) SELECT id, maskedFilename FROM files;",
  )?;
  tx.commit()?;
  Ok(Pruned { files, directories })
}

/// Rebuilds the database file without its free pages and empties the write-ahead log into it.
/// A reader holding a snapshot can keep the log from emptying; that is retried three times a
/// second apart and then left, since nothing is lost by it.
pub fn maintain(conn: &Connection) {
  debug!("Vacuuming...");
  if let Err(e) = conn.execute_batch("VACUUM;") {
    warn!(error = %e, "shared file cache not vacuumed");
  }
  debug!("Checkpointing...");
  for attempt in 1..=3 {
    match conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
      Ok((
        row.get::<_, i64>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)?,
      ))
    }) {
      Ok((0, log, done)) if log == done => return,
      Ok(_) if attempt < 3 => std::thread::sleep(Duration::from_secs(1)),
      Ok(_) => warn!("Failed to fully checkpoint the shares database after 3 attempts"),
      Err(e) => {
        warn!(error = %e, "shares database not checkpointed");
        return;
      }
    }
  }
}

/// Puts the files written with the row ids `ids` in the filename index, in place of what it
/// had for them.
pub fn index_files(tx: &Transaction, ids: &[i64], files: &[FileRow]) -> rusqlite::Result<()> {
  let mut delete = tx.prepare_cached("DELETE FROM filenames WHERE rowid = ?1")?;
  let mut insert =
    tx.prepare_cached("INSERT INTO filenames (rowid, maskedFilename) VALUES (?1, ?2)")?;
  for (id, file) in ids.iter().zip(files) {
    delete.execute([id])?;
    insert.execute(params![id, file.masked])?;
  }
  Ok(())
}

/// Removes the file or the directory named `masked`, a directory with everything under it,
/// from the index and the filename index. Returns how many file rows went.
pub fn remove(tx: &Transaction, masked: &str) -> rusqlite::Result<usize> {
  let prefix = format!("{masked}\\");
  tx.execute(
    "DELETE FROM filenames WHERE rowid IN (SELECT id FROM files WHERE maskedFilename = ?1 \
     OR (maskedFilename >= ?2 AND maskedFilename < ?3))",
    params![masked, prefix, upper(&prefix)],
  )?;
  let files = tx.execute(
    "DELETE FROM files WHERE maskedFilename = ?1 OR (maskedFilename >= ?2 AND maskedFilename < ?3)",
    params![masked, prefix, upper(&prefix)],
  )?;
  tx.execute(
    "DELETE FROM directories WHERE name = ?1 OR (name >= ?2 AND name < ?3)",
    params![masked, prefix, upper(&prefix)],
  )?;
  Ok(files)
}

/// Removes what is under the directory `masked`, the directory itself included, that was not
/// written at or after `timestamp`: what a rescan of that directory did not see.
pub fn prune_under(tx: &Transaction, masked: &str, timestamp: i64) -> rusqlite::Result<usize> {
  let prefix = format!("{masked}\\");
  let range = params![masked, prefix, upper(&prefix), timestamp];
  tx.execute(
    "DELETE FROM filenames WHERE rowid IN (SELECT id FROM files WHERE (maskedFilename = ?1 \
     OR (maskedFilename >= ?2 AND maskedFilename < ?3)) AND timestamp < ?4)",
    range,
  )?;
  let files = tx.execute(
    "DELETE FROM files WHERE (maskedFilename = ?1 OR (maskedFilename >= ?2 AND \
     maskedFilename < ?3)) AND timestamp < ?4",
    range,
  )?;
  tx.execute(
    "DELETE FROM directories WHERE (name = ?1 OR (name >= ?2 AND name < ?3)) AND timestamp < ?4",
    range,
  )?;
  Ok(files)
}

#[cfg(test)]
mod tests {
  use super::super::tests::{file, index};
  use super::*;
  use crate::testing::logged;

  #[tokio::test]
  async fn two_local_files_with_one_remote_name_are_warned_about() {
    let db = index().await;
    let (_, text) = db
      .write(|conn| {
        Ok(logged(|| {
          let tx = conn.transaction().unwrap();
          let mut first = file("Music\\a_b\\01.mp3");
          first.original = b"/music/a\\b/01.mp3".to_vec();
          let mut second = file("Music\\a_b\\01.mp3");
          second.original = b"/music/a_b/01.mp3".to_vec();
          for rows in [vec![first], vec![second]] {
            let directory = DirectoryRows {
              name: "Music\\a_b".to_string(),
              files: rows,
            };
            write_directory(&tx, 7, &directory).unwrap();
          }
          tx.commit().unwrap();
        }))
      })
      .await
      .unwrap();
    assert!(
      text.contains("two local files share one remote name"),
      "{text}"
    );
    let (_, again) = db
      .write(|conn| {
        Ok(logged(|| {
          let tx = conn.transaction().unwrap();
          let mut same = file("Music\\a_b\\01.mp3");
          same.original = b"/music/a_b/01.mp3".to_vec();
          let directory = DirectoryRows {
            name: "Music\\a_b".to_string(),
            files: vec![same],
          };
          write_directory(&tx, 8, &directory).unwrap();
          tx.commit().unwrap();
        }))
      })
      .await
      .unwrap();
    assert!(
      !again.contains("two local files share one remote name"),
      "a later scan of the same file is no collision: {again}"
    );
  }
}
