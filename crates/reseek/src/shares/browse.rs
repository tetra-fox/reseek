//! The browse cache: our whole share listing as the encoded message a peer that browses us is
//! sent, kept in a file so a browse is streamed from it instead of being built each time.

use std::path::{Path, PathBuf};
use std::time::Instant;

use soulseek_proto::PeerMessage;
use soulseek_proto::peer::BrowseResponse;
use tracing::{debug, error, info, warn};

use super::repository;
use crate::db::{Database, DbError};

/// The name of the browse cache file.
const FILE: &str = "browse.cache";

/// How the files a rebuild writes before it renames one into place begin.
const TEMPORARY: &str = "browse.cache.tmp-";

/// The file the browse cache is kept in, in the cache directory.
pub fn path(cache: &Path) -> PathBuf {
  cache.join(FILE)
}

/// Why the browse cache was not written.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
  /// the index could not be read
  #[error(transparent)]
  Db(#[from] DbError),
  /// the file could not be written
  #[error(transparent)]
  Io(#[from] std::io::Error),
  /// encoding stopped without finishing
  #[error("encoding the browse response did not finish: {0}")]
  Task(String),
}

/// Writes the listing in `db` to the browse cache at `file`, through a file of its own beside
/// it that replaces it whole, so a peer never reads half of one.
pub async fn rebuild(db: &Database, file: &Path) -> Result<(), CacheError> {
  let started = Instant::now();
  info!("Warming browse response cache...");
  let directories = db.read(|conn| repository::browse(conn, None)).await?;
  let file = file.to_path_buf();
  tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
    let message = PeerMessage::BrowseResponse(BrowseResponse {
      directories,
      locked_directories: Vec::new(),
    });
    // a name of its own, so rebuilds that overlap never write into each other's file
    let temporary = file.with_file_name(format!("{TEMPORARY}{:016x}", rand::random::<u64>()));
    let written = std::fs::write(&temporary, message.encode())
      .and_then(|()| std::fs::rename(&temporary, &file));
    if written.is_err()
      && let Err(e) = std::fs::remove_file(&temporary)
      && e.kind() != std::io::ErrorKind::NotFound
    {
      warn!(path = %temporary.display(), error = %e, "unfinished browse cache not removed");
    }
    written
  })
  .await
  .map_err(|e| CacheError::Task(e.to_string()))??;
  info!(
    ms = started.elapsed().as_millis() as u64,
    "Browse response cached successfully"
  );
  Ok(())
}

/// Deletes the browse cache a last run left in `cache`, which may list shares configured
/// differently, and any file a rebuild was writing when that run stopped.
pub async fn remove_stale(cache: &Path) {
  match tokio::fs::remove_file(path(cache)).await {
    Ok(()) => debug!("browse cache of the last run removed"),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
    Err(e) => warn!(error = %e, "browse cache of the last run not removed"),
  }
  let mut entries = match tokio::fs::read_dir(cache).await {
    Ok(entries) => entries,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
    Err(e) => return warn!(error = %e, "cache directory not read for unfinished browse caches"),
  };
  loop {
    let entry = match entries.next_entry().await {
      Ok(Some(entry)) => entry,
      Ok(None) => return,
      Err(e) => return warn!(error = %e, "cache directory not read for unfinished browse caches"),
    };
    if !entry.file_name().to_string_lossy().starts_with(TEMPORARY) {
      continue;
    }
    let leftover = entry.path();
    match tokio::fs::remove_file(&leftover).await {
      Ok(()) => {
        debug!(path = %leftover.display(), "unfinished browse cache of the last run removed")
      }
      Err(e) => {
        warn!(path = %leftover.display(), error = %e, "unfinished browse cache of the last run not removed")
      }
    }
  }
}

/// [`rebuild`], logging a failure.
pub async fn refresh(db: &Database, file: &Path) {
  if let Err(e) = rebuild(db, file).await {
    error!(error = %e, "Error caching browse response");
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::shares::repository::tests::{index, write};

  #[tokio::test]
  async fn the_cache_holds_the_listing_as_one_encoded_message() {
    let db = index().await;
    db.write(|conn| {
      write(conn, 1, &[("m", &[]), ("m\\a", &["m\\a\\1.mp3"])]);
      Ok(())
    })
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = path(dir.path());
    rebuild(&db, &file).await.unwrap();
    let bytes = std::fs::read(&file).unwrap();
    let PeerMessage::BrowseResponse(read) = PeerMessage::decode(&bytes).unwrap() else {
      panic!("not a browse response");
    };
    let names: Vec<&str> = read.directories.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["m", "m\\a"]);
    assert_eq!(read.directories[1].files[0].filename.as_str(), "1.mp3");
    assert!(read.locked_directories.is_empty());
  }

  #[tokio::test]
  async fn a_stale_cache_and_unfinished_rebuilds_are_removed_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let kept = dir.path().join("shares.db");
    for name in [
      FILE,
      "browse.cache.tmp-00000000000000ff",
      "browse.cache.tmp-1",
      "shares.db",
    ] {
      std::fs::write(dir.path().join(name), "x").unwrap();
    }
    remove_stale(dir.path()).await;
    let left: Vec<_> = std::fs::read_dir(dir.path())
      .unwrap()
      .map(|entry| entry.unwrap().path())
      .collect();
    assert_eq!(left, [kept]);
  }
}
