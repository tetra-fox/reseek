//! The browse cache: our whole share listing as the encoded message a peer that browses us is
//! sent, kept in a file so a browse is streamed from it instead of being built each time.

use std::path::{Path, PathBuf};
use std::time::Instant;

use soulseek_proto::PeerMessage;
use soulseek_proto::peer::BrowseResponse;
use tracing::{error, info, warn};

use super::repository;
use crate::db::{Database, DbError};

/// The name of the browse cache file.
const FILE: &str = "browse.cache";

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
    let temporary = file.with_file_name(format!("{FILE}.tmp-{:016x}", rand::random::<u64>()));
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
}
