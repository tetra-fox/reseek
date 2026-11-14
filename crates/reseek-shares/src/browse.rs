//! The browse caches: our share listing as the encoded message a peer that browses us is sent,
//! kept in a file so a browse is streamed from it instead of being built each time. Each view of
//! the shares has a file of its own, written the first time a peer with that view browses. The
//! files of the views that hide something carry a generation, which a change of who sees what
//! moves on, so the files written before it are never read again and can be removed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use soulseek_proto::PeerMessage;
use soulseek_proto::peer::{BrowseResponse, Directory};
use tracing::{debug, warn};

use super::hosts::Index;
use super::repository;
use super::visibility::View;
use crate::db::DbError;

/// The name of the browse cache file of the view that hides nothing.
const FILE: &str = "browse.cache";

/// How the names of the browse cache files begin and end.
const PREFIX: &str = "browse.";
const SUFFIX: &str = ".cache";

/// How the files a rebuild writes before it renames one into place begin.
const TEMPORARY: &str = "browse.cache.tmp-";

/// The file the browse cache of `view` is kept in, in the cache directory, in the generation
/// `generation` of the views that hide something.
pub fn path(cache: &Path, view: &View, generation: u64) -> PathBuf {
  if view.is_everything() {
    cache.join(FILE)
  } else {
    cache.join(format!("{PREFIX}{generation}-{}{SUFFIX}", view.key()))
  }
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

/// The listings of several hosts as one: a directory more than one has is listed once with the
/// files of each, in host order; directories are ordered by name.
pub fn merge(listings: Vec<Vec<Directory>>) -> Vec<Directory> {
  let mut tree: BTreeMap<String, Directory> = BTreeMap::new();
  for directory in listings.into_iter().flatten() {
    match tree.get_mut(directory.name.as_str()) {
      Some(listed) => listed.files.extend(directory.files),
      None => {
        tree.insert(directory.name.as_str().to_string(), directory);
      }
    }
  }
  tree.into_values().collect()
}

/// The listing of every index in `dbs`, or of the share `alias` in each, merged; an agent's
/// index that cannot be read is left out.
pub async fn listing(dbs: &[Arc<Index>], alias: Option<String>) -> Result<Vec<Directory>, DbError> {
  let mut listings = Vec::with_capacity(dbs.len());
  for db in dbs {
    let alias = alias.clone();
    match db
      .read(move |conn| repository::browse(conn, alias.as_deref()))
      .await
    {
      Ok(listing) => listings.push(listing),
      Err(e) => db.left_out(e)?,
    }
  }
  Ok(merge(listings))
}

/// A browse cache written beside the file it is for, still to be renamed over it.
pub struct Unplaced {
  /// the browse cache file it is for
  pub file: PathBuf,
  temporary: PathBuf,
  /// the number of the read of the shares it was made from
  pub read: u64,
}

impl Unplaced {
  /// Renames it over its file, which a peer then reads whole.
  pub async fn place(&self) -> std::io::Result<()> {
    let placed = tokio::fs::rename(&self.temporary, &self.file).await;
    if placed.is_err() {
      self.remove().await;
    }
    placed
  }

  /// Deletes it, leaving its file as it is.
  pub async fn discard(self) {
    self.remove().await;
  }

  async fn remove(&self) {
    if let Err(e) = tokio::fs::remove_file(&self.temporary).await
      && e.kind() != std::io::ErrorKind::NotFound
    {
      warn!(path = %self.temporary.display(), error = %e, "unfinished browse cache not removed");
    }
  }
}

/// Writes the listing of `dbs`, the local index and the relay agents', as `view` sees it, to a
/// file of its own beside the browse cache `file`, numbered `read`, the read of the shares it
/// is made from.
pub async fn write(
  dbs: &[Arc<Index>],
  file: &Path,
  view: &View,
  read: u64,
) -> Result<Unplaced, CacheError> {
  let mut directories = listing(dbs, None).await?;
  directories.retain(|directory| !view.hides(directory.name.as_str()));
  // a name of its own, so writes that overlap never write into each other's file
  let temporary = file.with_file_name(format!("{TEMPORARY}{:016x}", rand::random::<u64>()));
  let unplaced = Unplaced {
    file: file.to_path_buf(),
    temporary: temporary.clone(),
    read,
  };
  let written = tokio::task::spawn_blocking(move || {
    let message = PeerMessage::BrowseResponse(BrowseResponse {
      directories,
      locked_directories: Vec::new(),
    });
    std::fs::write(&temporary, message.encode())
  })
  .await
  .map_err(|e| CacheError::Task(e.to_string()))
  .and_then(|written| written.map_err(CacheError::from));
  if let Err(e) = written {
    unplaced.discard().await;
    return Err(e);
  }
  Ok(unplaced)
}

/// Deletes the browse caches a last run left in `cache`, which may list shares configured
/// differently, and any file a rebuild was writing when that run stopped.
pub async fn remove_stale(cache: &Path) {
  remove(cache, |name| is_cache(name) || name.starts_with(TEMPORARY)).await;
}

/// Deletes the browse caches in `cache` of the views that hide something from generations
/// before `generation`.
pub async fn remove_older(cache: &Path, generation: u64) {
  let current = format!("{PREFIX}{generation}-");
  remove(cache, |name| {
    is_cache(name) && name != FILE && !name.starts_with(&current)
  })
  .await;
}

/// Whether the file `name` is a browse cache.
fn is_cache(name: &str) -> bool {
  name.starts_with(PREFIX) && name.ends_with(SUFFIX)
}

/// Deletes the files in `cache` whose names `stale` picks.
async fn remove(cache: &Path, stale: impl Fn(&str) -> bool) {
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
    if !stale(&entry.file_name().to_string_lossy()) {
      continue;
    }
    let leftover = entry.path();
    match tokio::fs::remove_file(&leftover).await {
      Ok(()) => debug!(path = %leftover.display(), "stale browse cache removed"),
      Err(e) => warn!(path = %leftover.display(), error = %e, "stale browse cache not removed"),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::repository::tests::{index, write};
  use soulseek_proto::RemoteFilename;

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
    let file = path(dir.path(), &View::everything(), 0);
    super::write(&[Index::local(db)], &file, &View::everything(), 1)
      .await
      .unwrap()
      .place()
      .await
      .unwrap();
    let bytes = std::fs::read(&file).unwrap();
    let PeerMessage::BrowseResponse(read) = PeerMessage::decode(&bytes).unwrap() else {
      panic!("not a browse response");
    };
    let names: Vec<&str> = read.directories.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["m", "m\\a"]);
    assert_eq!(read.directories[1].files[0].filename.as_str(), "1.mp3");
    assert!(read.locked_directories.is_empty());
  }

  #[test]
  fn hosts_with_the_same_directory_list_it_once_with_both_files() {
    let directory = |name: &str, files: &[&str]| Directory {
      name: RemoteFilename::from(name.to_string()),
      files: files
        .iter()
        .map(|f| soulseek_proto::peer::File {
          code: 1,
          filename: RemoteFilename::from(f.to_string()),
          size: 1,
          extension: String::new(),
          attributes: Vec::new(),
        })
        .collect(),
    };
    let merged = merge(vec![
      vec![directory("b", &["1"]), directory("m", &["local"])],
      vec![directory("a", &[]), directory("m", &["agent"])],
    ]);
    let listed: Vec<(&str, Vec<&str>)> = merged
      .iter()
      .map(|d| {
        (
          d.name.as_str(),
          d.files.iter().map(|f| f.filename.as_str()).collect(),
        )
      })
      .collect();
    assert_eq!(
      listed,
      [
        ("a", vec![]),
        ("b", vec!["1"]),
        ("m", vec!["local", "agent"])
      ]
    );
  }

  #[tokio::test]
  async fn a_stale_cache_and_unfinished_rebuilds_are_removed_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let kept = dir.path().join("shares.db");
    for name in [
      FILE,
      "browse.0123456789abcdef.cache",
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
