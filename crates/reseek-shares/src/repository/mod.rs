//! The share index in SQLite: the rows of the scans, directories and files, and the queries
//! that read them back as the network sees them; what a scan or a watched change writes is in
//! `writes`. Every function runs on a connection the caller holds, on a blocking thread.
//!
//! A "prefix" here is a remote directory name followed by `\`; rows under it are found with a
//! range on the name, which is exact, case-sensitive and uses the index.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use api_types::search::{FileAttribute, FileAttributeType};
use rusqlite::{Connection, OptionalExtension, params};
use soulseek_proto::RemoteFilename;
use soulseek_proto::peer::{self, Directory};

use crate::db::{self, Schema};

mod writes;

pub use writes::*;

/// `shares.<host>.db`: the scans, directories, files and the filename index.
pub static SCHEMA: Schema = db::schema!("shares", ["0001_initial"]);

/// One scan as recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scan {
  /// when it started, in epoch milliseconds; also its key
  pub started_at: i64,
  /// the share options it ran with, as JSON
  pub options: String,
  /// when it ended, `None` while it runs or when it never finished
  pub ended_at: Option<i64>,
  /// the index may have drifted from the disk since
  pub suspect: bool,
}

/// A file as the scan or a watched change read it from disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRow {
  /// the remote name
  pub masked: String,
  /// the local path's bytes
  pub original: Vec<u8>,
  /// bytes
  pub size: u64,
  /// the last write time, epoch milliseconds
  pub touched_at: i64,
  /// lower case, without the dot
  pub extension: String,
  /// in the order they are sent
  pub attributes: Vec<FileAttribute>,
}

/// A directory with the files directly in it, as one transaction writes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryRows {
  /// the remote name
  pub name: String,
  pub files: Vec<FileRow>,
}

/// What a file's row held when it was last written, for reusing its attributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
  /// bytes
  pub size: u64,
  /// the last write time, epoch milliseconds
  pub touched_at: i64,
  /// the attributes read then
  pub attributes: Vec<FileAttribute>,
}

/// The upper end of the range of names under `prefix`, which ends in `\`: the same text with
/// that `\` replaced by the next character, `]`.
fn upper(prefix: &str) -> String {
  debug_assert!(prefix.ends_with('\\'));
  let mut upper = prefix[..prefix.len() - 1].to_string();
  upper.push(']');
  upper
}

/// A local path's bytes, as the index keeps them.
pub fn path_bytes(path: &std::path::Path) -> Vec<u8> {
  #[cfg(unix)]
  {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
  }
  #[cfg(not(unix))]
  {
    path.to_string_lossy().into_owned().into_bytes()
  }
}

/// The local path whose bytes the index keeps.
pub fn bytes_path(bytes: Vec<u8>) -> PathBuf {
  #[cfg(unix)]
  {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
  }
  #[cfg(not(unix))]
  {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
  }
}

fn scan(row: &rusqlite::Row) -> rusqlite::Result<Scan> {
  Ok(Scan {
    started_at: row.get(0)?,
    options: row.get(1)?,
    ended_at: row.get(2)?,
    suspect: row.get(3)?,
  })
}

pub fn latest_scan(conn: &Connection) -> rusqlite::Result<Option<Scan>> {
  conn
    .query_row(
      "SELECT timestamp, options, \"end\", suspect FROM scans ORDER BY timestamp DESC LIMIT 1",
      [],
      scan,
    )
    .optional()
}

/// When the last scan that completed and is not suspect started, in epoch milliseconds.
pub fn latest_trusted_scan(conn: &Connection) -> rusqlite::Result<Option<i64>> {
  conn.query_row(
    "SELECT MAX(timestamp) FROM scans WHERE \"end\" IS NOT NULL AND suspect = 0",
    [],
    |row| row.get(0),
  )
}

/// The rows of the files directly in the directory `name`, by remote name.
pub fn known_files(conn: &Connection, name: &str) -> rusqlite::Result<HashMap<String, Known>> {
  let prefix = format!("{name}\\");
  let mut statement = conn.prepare_cached(
    "SELECT maskedFilename, size, touchedAt, attributeJson FROM files \
     WHERE maskedFilename >= ?1 AND maskedFilename < ?2",
  )?;
  let rows = statement.query_map(params![prefix, upper(&prefix)], |row| {
    Ok((
      row.get::<_, String>(0)?,
      row.get::<_, i64>(1)?,
      row.get::<_, i64>(2)?,
      row.get::<_, String>(3)?,
    ))
  })?;
  let mut known = HashMap::new();
  for row in rows {
    let (masked, size, touched_at, json) = row?;
    if masked[prefix.len()..].contains('\\') {
      continue;
    }
    known.insert(
      masked,
      Known {
        size: size.max(0).unsigned_abs(),
        touched_at,
        attributes: attributes(&json),
      },
    );
  }
  Ok(known)
}

/// The attributes a row's JSON holds; a row written by an older version that does not read is
/// taken as having none.
fn attributes(json: &str) -> Vec<FileAttribute> {
  serde_json::from_str(json).unwrap_or_default()
}

/// How many directories and files are in the index under the share `alias`, its own directory
/// counted, or in the whole index without one.
pub fn count(conn: &Connection, alias: Option<&str>) -> rusqlite::Result<(u32, u32)> {
  let to_u32 = |n: i64| u32::try_from(n).unwrap_or(u32::MAX);
  let Some(alias) = alias else {
    let directories: i64 = conn.query_row("SELECT COUNT(*) FROM directories", [], |r| r.get(0))?;
    let files: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
    return Ok((to_u32(directories), to_u32(files)));
  };
  let prefix = format!("{alias}\\");
  let directories: i64 = conn.query_row(
    "SELECT COUNT(*) FROM directories WHERE name = ?1 OR (name >= ?2 AND name < ?3)",
    params![alias, prefix, upper(&prefix)],
    |r| r.get(0),
  )?;
  let files: i64 = conn.query_row(
    "SELECT COUNT(*) FROM files WHERE maskedFilename >= ?1 AND maskedFilename < ?2",
    params![prefix, upper(&prefix)],
    |r| r.get(0),
  )?;
  Ok((to_u32(directories), to_u32(files)))
}

/// A stored attribute as the network sends it.
fn wire_attribute(attribute: &FileAttribute) -> peer::FileAttribute {
  use soulseek_proto::FileAttributeType as Wire;
  let attribute_type = match attribute.attribute_type {
    FileAttributeType::BitRate => Wire::BitRate,
    FileAttributeType::Length => Wire::Duration,
    FileAttributeType::VariableBitRate => Wire::VariableBitRate,
    FileAttributeType::SampleRate => Wire::SampleRate,
    FileAttributeType::BitDepth => Wire::BitDepth,
    FileAttributeType::Unknown(code) => Wire::Unknown(code),
  };
  peer::FileAttribute {
    attribute_type,
    value: attribute.value,
  }
}

/// A file row as the network sees it, named `filename`.
pub(super) fn shared_file(filename: &str, size: i64, extension: String, json: &str) -> peer::File {
  peer::File {
    code: 1,
    filename: RemoteFilename::from(filename),
    size: size.max(0).unsigned_abs(),
    extension,
    attributes: attributes(json).iter().map(wire_attribute).collect(),
  }
}

/// The part of a remote name before its last `\`, and the bare name after it.
fn split(masked: &str) -> (&str, &str) {
  masked.rsplit_once('\\').unwrap_or(("", masked))
}

/// Every directory of the index, or of the share `alias` with its own directory, ordered by
/// name, each with the files directly in it under their bare names, ordered by name. A
/// directory that holds only directories is listed too, empty.
pub fn browse(conn: &Connection, alias: Option<&str>) -> rusqlite::Result<Vec<Directory>> {
  let mut tree: BTreeMap<String, Vec<peer::File>> = BTreeMap::new();
  let prefix = alias.map(|alias| format!("{alias}\\"));
  {
    let mut statement = match &prefix {
      Some(_) => conn.prepare(
        "SELECT name FROM directories WHERE name = ?1 OR (name >= ?2 AND name < ?3) \
         ORDER BY name",
      )?,
      None => conn.prepare("SELECT name FROM directories ORDER BY name")?,
    };
    let mut rows = match (&prefix, alias) {
      (Some(prefix), Some(alias)) => statement.query(params![alias, prefix, upper(prefix)])?,
      _ => statement.query([])?,
    };
    while let Some(row) = rows.next()? {
      tree.insert(row.get(0)?, Vec::new());
    }
  }
  let mut statement = match &prefix {
    Some(_) => conn.prepare(
      "SELECT maskedFilename, size, extension, attributeJson FROM files \
       WHERE maskedFilename >= ?1 AND maskedFilename < ?2 ORDER BY maskedFilename",
    )?,
    None => conn.prepare(
      "SELECT maskedFilename, size, extension, attributeJson FROM files \
       ORDER BY maskedFilename",
    )?,
  };
  let mut rows = match &prefix {
    Some(prefix) => statement.query(params![prefix, upper(prefix)])?,
    None => statement.query([])?,
  };
  while let Some(row) = rows.next()? {
    let masked: String = row.get(0)?;
    let (directory, name) = split(&masked);
    let file = shared_file(name, row.get(1)?, row.get(2)?, &row.get::<_, String>(3)?);
    tree.entry(directory.to_string()).or_default().push(file);
  }
  Ok(
    tree
      .into_iter()
      .map(|(name, files)| Directory {
        name: RemoteFilename::from(name),
        files,
      })
      .collect(),
  )
}

/// The directory `name` and every directory under it, ordered by name, each with the files
/// directly in it under their bare names; a single empty entry named `name` when the index has
/// no such directory.
pub fn folder(conn: &Connection, name: &str) -> rusqlite::Result<Vec<Directory>> {
  let prefix = format!("{name}\\");
  let mut tree: BTreeMap<String, Vec<peer::File>> = BTreeMap::new();
  {
    let mut statement = conn.prepare_cached(
      "SELECT name FROM directories WHERE name = ?1 OR (name >= ?2 AND name < ?3)",
    )?;
    let mut rows = statement.query(params![name, prefix, upper(&prefix)])?;
    while let Some(row) = rows.next()? {
      tree.insert(row.get(0)?, Vec::new());
    }
  }
  let mut statement = conn.prepare_cached(
    "SELECT maskedFilename, size, extension, attributeJson FROM files \
     WHERE maskedFilename >= ?1 AND maskedFilename < ?2 ORDER BY maskedFilename",
  )?;
  let mut rows = statement.query(params![prefix, upper(&prefix)])?;
  while let Some(row) = rows.next()? {
    let masked: String = row.get(0)?;
    let (directory, file) = split(&masked);
    let file = shared_file(file, row.get(1)?, row.get(2)?, &row.get::<_, String>(3)?);
    tree.entry(directory.to_string()).or_default().push(file);
  }
  if tree.is_empty() {
    tree.insert(name.to_string(), Vec::new());
  }
  Ok(
    tree
      .into_iter()
      .map(|(name, files)| Directory {
        name: RemoteFilename::from(name),
        files,
      })
      .collect(),
  )
}

/// The local path and size of the file whose remote name is exactly `masked`.
pub fn find_file(conn: &Connection, masked: &str) -> rusqlite::Result<Option<(PathBuf, u64)>> {
  conn
    .query_row(
      "SELECT originalFilename, size FROM files WHERE maskedFilename = ?1",
      [masked],
      |row| {
        Ok((
          bytes_path(row.get(0)?),
          row.get::<_, i64>(1)?.max(0).unsigned_abs(),
        ))
      },
    )
    .optional()
}

pub fn has_directory(conn: &Connection, masked: &str) -> rusqlite::Result<bool> {
  conn
    .query_row(
      "SELECT 1 FROM directories WHERE name = ?1",
      [masked],
      |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
}

#[cfg(test)]
pub(super) mod tests {
  use super::*;

  /// An empty index in memory.
  pub async fn index() -> crate::db::Database {
    crate::db::Database::memory(&SCHEMA).await
  }

  /// A file row of `masked` with no attributes.
  pub fn file(masked: &str) -> FileRow {
    FileRow {
      masked: masked.to_string(),
      original: format!("/local/{masked}").into_bytes(),
      size: 10,
      touched_at: 1,
      extension: masked.rsplit_once('.').map_or("", |(_, e)| e).to_string(),
      attributes: Vec::new(),
    }
  }

  /// Writes `directories` with their files, stamped `timestamp`, and indexes the files.
  pub fn write(conn: &mut Connection, timestamp: i64, directories: &[(&str, &[&str])]) {
    let tx = conn.transaction().unwrap();
    for (name, files) in directories {
      let rows = DirectoryRows {
        name: name.to_string(),
        files: files.iter().map(|f| file(f)).collect(),
      };
      let ids = write_directory(&tx, timestamp, &rows).unwrap();
      index_files(&tx, &ids, &rows.files).unwrap();
    }
    tx.commit().unwrap();
  }

  fn names(directories: &[Directory]) -> Vec<(String, Vec<String>)> {
    directories
      .iter()
      .map(|d| {
        (
          d.name.as_str().to_string(),
          d.files
            .iter()
            .map(|f| f.filename.as_str().to_string())
            .collect(),
        )
      })
      .collect()
  }

  #[tokio::test]
  async fn browse_lists_every_directory_in_name_order_with_bare_names() {
    let db = index().await;
    db.write(|conn| {
      write(
        conn,
        5,
        &[
          ("music\\b", &["music\\b\\2.mp3", "music\\b\\1.mp3"]),
          ("music", &[]),
          ("music\\a", &[]),
          ("music\\a\\x", &["music\\a\\x\\y.flac"]),
          ("my_music", &["my_music\\z.mp3"]),
          ("myXmusic", &["myXmusic\\w.mp3"]),
        ],
      );
      let all = names(&browse(conn, None)?);
      assert_eq!(
        all,
        [
          ("music".to_string(), vec![]),
          ("music\\a".to_string(), vec![]),
          ("music\\a\\x".to_string(), vec!["y.flac".to_string()]),
          (
            "music\\b".to_string(),
            vec!["1.mp3".to_string(), "2.mp3".to_string()]
          ),
          ("myXmusic".to_string(), vec!["w.mp3".to_string()]),
          ("my_music".to_string(), vec!["z.mp3".to_string()]),
        ]
      );
      let one = names(&browse(conn, Some("my_music"))?);
      assert_eq!(one, [("my_music".to_string(), vec!["z.mp3".to_string()])]);
      assert_eq!(count(conn, Some("music"))?, (4, 3), "the root counts");
      assert_eq!(count(conn, Some("my_music"))?, (1, 1), "_ is no wildcard");
      assert_eq!(count(conn, Some("Music"))?, (0, 0), "names are exact");
      assert_eq!(count(conn, None)?, (6, 5));
      Ok(())
    })
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn folder_contents_list_each_directory_under_it_and_nothing_beside_it() {
    let db = index().await;
    db.write(|conn| {
      write(
        conn,
        5,
        &[
          ("m\\A", &["m\\A\\1.mp3"]),
          ("m\\A\\cd2", &["m\\A\\cd2\\2.mp3"]),
          ("m\\AB", &["m\\AB\\3.mp3"]),
        ],
      );
      assert_eq!(
        names(&folder(conn, "m\\A")?),
        [
          ("m\\A".to_string(), vec!["1.mp3".to_string()]),
          ("m\\A\\cd2".to_string(), vec!["2.mp3".to_string()]),
        ]
      );
      assert_eq!(
        names(&folder(conn, "m\\none")?),
        [("m\\none".to_string(), vec![])],
        "an unknown directory is one empty entry"
      );
      Ok(())
    })
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn a_finished_scan_prunes_what_it_did_not_see_and_reindexes() {
    let db = index().await;
    db.write(|conn| {
      write(conn, 1, &[("m", &["m\\old.mp3", "m\\kept.mp3"])]);
      write(conn, 2, &[("m", &["m\\kept.mp3"])]);
      let pruned = finish_scan(conn, 2)?;
      assert_eq!(
        pruned,
        Pruned {
          files: 1,
          directories: 0
        }
      );
      let indexed: i64 = conn.query_row("SELECT COUNT(*) FROM filenames", [], |r| r.get(0))?;
      assert_eq!(indexed, 1);
      assert_eq!(
        find_file(conn, "m\\kept.mp3")?,
        Some((PathBuf::from("/local/m\\kept.mp3"), 10))
      );
      assert_eq!(find_file(conn, "M\\kept.mp3")?, None, "lookups are exact");
      Ok(())
    })
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn removing_a_directory_takes_everything_under_it_and_only_that() {
    let db = index().await;
    db.write(|conn| {
      write(
        conn,
        1,
        &[
          ("m\\a", &["m\\a\\1.mp3"]),
          ("m\\a\\b", &["m\\a\\b\\2.mp3"]),
          ("m\\ab", &["m\\ab\\3.mp3"]),
        ],
      );
      let tx = conn.transaction()?;
      assert_eq!(remove(&tx, "m\\a")?, 2);
      tx.commit()?;
      assert_eq!(count(conn, None)?, (1, 1));
      let indexed: i64 = conn.query_row("SELECT COUNT(*) FROM filenames", [], |r| r.get(0))?;
      assert_eq!(indexed, 1);
      Ok(())
    })
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn scans_are_recorded_and_read_back() {
    let db = index().await;
    db.write(|conn| {
      insert_scan(conn, 10, "{}")?;
      insert_scan(conn, 20, "{\"a\":1}")?;
      end_scan(conn, 20, 25)?;
      mark_latest_suspect(conn)?;
      let latest = latest_scan(conn)?.unwrap();
      assert_eq!(latest.ended_at, Some(25));
      assert!(latest.suspect);
      assert_eq!(
        latest_trusted_scan(conn)?,
        None,
        "the ended scan is suspect"
      );
      end_scan(conn, 10, 12)?;
      assert_eq!(latest_trusted_scan(conn)?, Some(10));
      Ok(())
    })
    .await
    .unwrap();
  }
}
