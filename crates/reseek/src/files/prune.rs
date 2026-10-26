//! Deleting files left in the download directories for longer than their retention, every 30
//! minutes. Links are never followed or deleted, hidden entries are left alone, and a directory
//! emptied by the pruning goes too; the directory itself always stays.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tracing::{Instrument, debug, info_span, warn};

use super::Files;
use super::listing::is_system;
use super::writing::Writing;

/// How often old files are looked for.
const EVERY: Duration = Duration::from_secs(30 * 60);

/// What one pruning of a directory did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Pruned {
  /// files deleted
  pub deleted: u32,
  /// files that could not be deleted
  pub errors: u32,
}

/// Prunes both download directories every 30 minutes until the daemon stops.
pub(super) fn spawn(files: Arc<Files>, shutdown: tokio_util::sync::CancellationToken) {
  let task = async move {
    let mut every = tokio::time::interval(EVERY);
    loop {
      tokio::select! {
        () = shutdown.cancelled() => return,
        _ = every.tick() => {}
      }
      let retention = files.config.current().values.retention.files.clone();
      for (age, dir) in [
        (retention.incomplete, files.incomplete.clone()),
        (retention.complete, files.downloads.clone()),
      ] {
        let Some(age) = age else {
          continue;
        };
        let writing = files.writing.clone();
        let pruned = tokio::task::spawn_blocking(move || {
          debug!(dir = %dir.display(), minutes = age.as_secs() / 60, "Pruning files older than the retention");
          let pruned = prune(&dir, age, SystemTime::now(), &writing);
          debug!(dir = %dir.display(), deleted = pruned.deleted, errors = pruned.errors, "Pruning complete");
        })
        .await;
        if let Err(e) = pruned {
          warn!(error = %e, "file pruning did not finish");
        }
      }
    }
  };
  tokio::spawn(task.instrument(info_span!("file_pruning")));
}

/// Deletes the files under `dir` last written `age` or longer before `now`, except those
/// being written.
pub(super) fn prune(dir: &Path, age: Duration, now: SystemTime, writing: &Writing) -> Pruned {
  let mut pruned = Pruned::default();
  let root = match std::fs::canonicalize(dir) {
    Ok(root) => root,
    Err(e) => {
      warn!(dir = %dir.display(), error = %e, "Failed to prune files in directory");
      return pruned;
    }
  };
  let cutoff = now.checked_sub(age).unwrap_or(SystemTime::UNIX_EPOCH);
  if let Err(e) = prune_in(&root, cutoff, writing, &mut pruned) {
    warn!(dir = %dir.display(), error = %e, "Failed to prune files in directory");
  }
  pruned
}

/// Prunes `dir` and returns whether anything was deleted in it.
fn prune_in(
  dir: &Path,
  cutoff: SystemTime,
  writing: &Writing,
  pruned: &mut Pruned,
) -> std::io::Result<bool> {
  let mut deleted_here = false;
  for entry in std::fs::read_dir(dir)?.filter_map(Result::ok) {
    let name = entry.file_name();
    if name.to_string_lossy().starts_with('.') {
      continue;
    }
    let path = entry.path();
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
      continue;
    };
    if metadata.file_type().is_symlink() || is_system(&metadata) {
      continue;
    }
    if metadata.is_dir() {
      match prune_in(&path, cutoff, writing, pruned) {
        Ok(true) => {
          deleted_here = true;
          let empty = std::fs::read_dir(&path).is_ok_and(|mut entries| entries.next().is_none());
          if empty {
            match std::fs::remove_dir(&path) {
              Ok(()) => debug!(dir = %path.display(), "pruned an emptied directory"),
              Err(e) => debug!(dir = %path.display(), error = %e, "emptied directory not removed"),
            }
          }
        }
        Ok(false) => {}
        Err(e) => {
          debug!(dir = %path.display(), error = %e, "unreadable directory left out of pruning")
        }
      }
      continue;
    }
    let old = metadata.modified().is_ok_and(|at| at <= cutoff);
    if !old || writing.holds(&[&path]) {
      continue;
    }
    match std::fs::remove_file(&path) {
      Ok(()) => {
        pruned.deleted += 1;
        deleted_here = true;
      }
      Err(e) => {
        pruned.errors += 1;
        warn!(file = %path.display(), error = %e, "Failed to prune file");
      }
    }
  }
  Ok(deleted_here)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn old_files_go_with_the_directories_they_empty_and_links_stay() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("downloads");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(root.join("old album/disc")).unwrap();
    std::fs::create_dir_all(root.join("mixed")).unwrap();
    std::fs::create_dir_all(root.join("empty already")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    for file in [
      "old album/disc/1.flac",
      "mixed/keep.flac",
      "mixed/old.flac",
      ".hidden",
    ] {
      std::fs::write(root.join(file), b"x").unwrap();
    }
    std::fs::write(outside.join("theirs"), b"x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    let writing = Writing::default();
    let _held = writing.hold(vec![
      std::fs::canonicalize(&root)
        .unwrap()
        .join("mixed/keep.flac"),
    ]);
    // every file is older than a zero retention
    let later = SystemTime::now() + Duration::from_secs(60);
    let pruned = prune(&root, Duration::ZERO, later, &writing);
    assert_eq!(
      pruned,
      Pruned {
        deleted: 2,
        errors: 0
      }
    );
    assert!(!root.join("old album").exists(), "emptied directories go");
    assert!(
      root.join("mixed/keep.flac").exists(),
      "a file being written stays"
    );
    assert!(!root.join("mixed/old.flac").exists());
    assert!(
      root.join("empty already").exists(),
      "only what pruning emptied"
    );
    assert!(root.join(".hidden").exists());
    assert!(outside.join("theirs").exists(), "links are not followed");
    assert!(root.exists());
    let none = prune(
      &root,
      Duration::from_secs(3600),
      SystemTime::now(),
      &writing,
    );
    assert_eq!(none, Pruned::default(), "nothing is that old yet");
  }
}
