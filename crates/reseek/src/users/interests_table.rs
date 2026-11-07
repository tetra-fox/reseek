//! The interests an older `users.db` kept in its `interests` table, read once before the
//! migration that drops it, and written into the configuration file when nothing sets the lists
//! and the file can be written, else logged so they can be copied into the configuration.
//! TODO: remove with `0002_drop_interests` in the release after the one that ships it.

use std::collections::BTreeMap;
use std::path::Path;

use cascade::{Source, Watcher};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use tracing::{info, warn};

use super::{HATED, LIKED, store};
use crate::config::{Options, SoulseekInterests};
use crate::db::Location;

/// Moves the rows of the old table in `location`'s `users.db` into `config`, before the database
/// is opened and migrated. A database that cannot be read stops the start, since the migration
/// would drop the rows.
pub(super) async fn move_to_configuration(
  location: &Location,
  config: &Watcher<Options>,
) -> Result<(), String> {
  if location.volatile {
    return Ok(());
  }
  let path = location.data.join(format!("{}.db", store::SCHEMA.name));
  let read = tokio::task::spawn_blocking(move || stored(&path))
    .await
    .map_err(|e| format!("users.db: {e}"))?
    .map_err(|e| format!("users.db: reading the interests to move: {e}"))?;
  let Some(old) = read.filter(|old| !old.liked.is_empty() || !old.hated.is_empty()) else {
    return Ok(());
  };
  let refused = match write(config, &old).await {
    Ok(file) => {
      info!(
        file,
        liked = old.liked.len(),
        hated = old.hated.len(),
        "interests kept in users.db moved into the configuration"
      );
      return Ok(());
    }
    Err(reason) => reason,
  };
  warn!(
    liked = ?old.liked,
    hated = ?old.hated,
    reason = refused,
    "interests kept in users.db are not in the configuration; set {LIKED} and {HATED} to keep them"
  );
  Ok(())
}

/// The rows of the old table, lowercased and trimmed as they were sent, the first of a repeat
/// kept; none when the file or the table is not there.
fn stored(path: &Path) -> rusqlite::Result<Option<SoulseekInterests>> {
  if !path.exists() {
    return Ok(None);
  }
  let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
  let conn = Connection::open_with_flags(path, flags)?;
  let table: Option<String> = conn
    .query_row(
      "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'interests'",
      [],
      |row| row.get(0),
    )
    .optional()?;
  if table.is_none() {
    return Ok(None);
  }
  let mut interests = SoulseekInterests::default();
  let mut statement = conn.prepare("SELECT interest, liked FROM interests ORDER BY rowid")?;
  let rows = statement.query_map([], |row| {
    Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
  })?;
  for row in rows {
    let (interest, liked) = row?;
    let interest = interest.trim().to_lowercase();
    if interest.is_empty()
      || interests.liked.contains(&interest)
      || interests.hated.contains(&interest)
    {
      continue;
    }
    let list = if liked {
      &mut interests.liked
    } else {
      &mut interests.hated
    };
    list.push(interest);
  }
  Ok(Some(interests))
}

/// Writes `old` into the file a write of the lists goes to, and names it; refused, with why,
/// when a layer sets either list already or the file cannot be written.
async fn write(config: &Watcher<Options>, old: &SoulseekInterests) -> Result<String, String> {
  let current = config.current();
  let set_already = [LIKED, HATED]
    .iter()
    .any(|key| !matches!(current.sources.get(*key), None | Some(Source::Default)));
  if set_already {
    return Err("the configuration sets the interests already".to_string());
  }
  let file = current
    .file_for(LIKED)
    .and_then(|name| current.files.iter().find(|f| f.name == name))
    .ok_or("the configuration has no files")?;
  cascade::writability(&file.path, current.managed_by.as_deref())
    .map_err(|reason| format!("{} cannot be written: {reason}", file.name))?;
  let set: BTreeMap<String, serde_json::Value> = [(LIKED, &old.liked), (HATED, &old.hated)]
    .into_iter()
    .filter(|(_, list)| !list.is_empty())
    .map(|(key, list)| (key.to_string(), serde_json::json!(list)))
    .collect();
  config
    .patch_file(&file.name, file.revision.as_deref(), set, Vec::new())
    .await
    .map_err(|e| format!("writing {} failed: {e}", file.name))?;
  Ok(file.name.clone())
}

#[cfg(test)]
mod tests {
  use std::ffi::OsString;

  use super::*;
  use crate::config::load::{self, ConfigFiles, Sources};
  use crate::db::{self, Database, Schema};
  use crate::paths::Defaults;

  /// `users.db` as it was before the interests moved.
  static OLD: Schema = db::schema!("users", ["0001_initial"]);

  /// A watcher over `config.toml` in `dir` holding `text`, with the environment `env`.
  fn watching(dir: &Path, text: &str, env: &[(&str, &str)]) -> Watcher<Options> {
    std::fs::write(dir.join("config.toml"), text).unwrap();
    let sources = Sources {
      env: env
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect(),
      files: ConfigFiles::new(&[], dir),
      cli: Vec::new(),
    };
    Watcher::new(load::layers(&sources, &Defaults::new(Some(dir)), None)).unwrap()
  }

  /// An old `users.db` in `location` holding `rows` of `(interest, liked)`.
  async fn old_database(location: &Location, rows: &'static [(&'static str, bool)]) {
    let old = Database::open(&OLD, location).await.unwrap();
    old
      .write(move |conn| {
        for (interest, liked) in rows {
          conn.execute(
            "INSERT INTO interests (interest, liked) VALUES (?1, ?2)",
            rusqlite::params![interest, liked],
          )?;
        }
        Ok(())
      })
      .await
      .unwrap();
  }

  #[tokio::test]
  async fn old_rows_move_into_a_writable_file_that_sets_no_list_and_the_table_goes() {
    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    old_database(
      &location,
      &[("Jazz", true), ("polka", false), ("jazz", false)],
    )
    .await;
    let config = watching(dir.path(), "", &[]);
    move_to_configuration(&location, &config).await.unwrap();
    let interests = &config.current().values.soulseek.interests;
    assert_eq!(interests.liked, ["jazz"]);
    assert_eq!(interests.hated, ["polka"]);

    let migrated = Database::open(&store::SCHEMA, &location).await.unwrap();
    let left: i64 = migrated
      .read(|conn| {
        conn.query_row(
          "SELECT count(*) FROM sqlite_master WHERE name = 'interests'",
          [],
          |row| row.get(0),
        )
      })
      .await
      .unwrap();
    assert_eq!(left, 0);
  }

  #[tokio::test]
  async fn old_rows_are_not_moved_over_lists_another_layer_sets() {
    let dir = tempfile::tempdir().unwrap();
    let location = Location::on_disk(dir.path());
    old_database(&location, &[("jazz", true)]).await;
    let config = watching(dir.path(), "", &[("RESEEK_SLSK_INTERESTS_HATED", "polka")]);
    move_to_configuration(&location, &config).await.unwrap();
    let interests = &config.current().values.soulseek.interests;
    assert!(interests.liked.is_empty());
    let written = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert_eq!(written, "");
  }
}
