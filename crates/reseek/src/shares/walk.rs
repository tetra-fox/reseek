//! The walk of the shares: which directories a scan takes, and reading one directory's files
//! into rows. Hidden and system entries, links unless they are followed, directories it may
//! not read, and, with everything under them, excluded directories and directories a filter
//! matches are left out. A file whose size and modification time match its row keeps the
//! attributes read before, unless the scan is forced.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use soulseek_client::LogLimiter;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use super::attributes;
use super::repository::{self, DirectoryRows, FileRow, Known};
use super::scanner::{ScanError, Settings};
use super::share::{self, CASE_SENSITIVE, Share};
use crate::db::Database;
use crate::entries;

/// Directories the walk cannot read, which a large share can hold many of.
static UNREADABLE: LogLimiter = LogLimiter::new();

/// The key that tells two directory paths apart by this system's case rule.
fn key(path: &Path) -> PathBuf {
  if CASE_SENSITIVE {
    path.to_path_buf()
  } else {
    PathBuf::from(path.to_string_lossy().to_lowercase())
  }
}

/// The directories to scan under `root`, `root` first, by the walk's rules, and how many were
/// left out because they are excluded or filtered. `root` is always taken when `with_root`.
pub(super) fn directories_under(
  settings: &Settings,
  root: &Path,
  with_root: bool,
  cancel: &CancellationToken,
  seen: &mut HashSet<PathBuf>,
) -> (Vec<PathBuf>, u32) {
  let mut found = Vec::new();
  let mut left_out = 0u32;
  if with_root && seen.insert(key(root)) {
    found.push(root.to_path_buf());
  }
  let mut canonical: HashSet<PathBuf> = HashSet::new();
  if settings.follow_symlinks
    && let Ok(root) = root.canonicalize()
  {
    canonical.insert(root);
  }
  let walk = walkdir::WalkDir::new(root)
    .follow_links(settings.follow_symlinks)
    // which of two ways to one directory wins should not depend on the order the system lists
    .sort_by_file_name()
    .into_iter()
    .filter_entry(|entry| {
      if entry.depth() == 0 {
        return true;
      }
      let metadata = entry.metadata().ok();
      if !entry.file_type().is_dir() || entries::left_out(entry.file_name(), metadata.as_ref()) {
        return false;
      }
      let path = entry.path();
      if share::excluded(&settings.shares, path) {
        debug!(
          directory = %path.display(),
          "Excluding directory because it is a subdirectory of an excluded share"
        );
        left_out += 1;
        return false;
      }
      if settings.filtered(path) {
        debug!(
          directory = %path.display(),
          "Excluding directory because it matches one or more share filters"
        );
        left_out += 1;
        return false;
      }
      // a directory reached a second time, through a link or to one, would be indexed twice
      // under two names, or walked forever
      if settings.follow_symlinks && !path.canonicalize().is_ok_and(|real| canonical.insert(real)) {
        return false;
      }
      true
    });
  for entry in walk {
    if cancel.is_cancelled() {
      break;
    }
    // a directory that cannot be read is left out, with everything under it. walkdir yields
    // the directory itself before it fails to list it, so it is taken back out here
    let entry = match entry {
      Ok(entry) => entry,
      Err(e) => {
        if let Some(path) = e.path() {
          found.retain(|taken| taken != path);
          seen.remove(&key(path));
        }
        if let Some(suppressed) = UNREADABLE.allow() {
          warn!(error = %e, suppressed, "a shared directory cannot be read; it is left out");
        }
        continue;
      }
    };
    if entry.depth() > 0 && seen.insert(key(entry.path())) {
      found.push(entry.into_path());
    }
  }
  (found, left_out)
}

/// Every directory to scan, each share's own first, and how many were left out.
pub(super) fn directories(settings: &Settings, cancel: &CancellationToken) -> (Vec<PathBuf>, u32) {
  let mut seen = HashSet::new();
  let mut all = Vec::new();
  let mut left_out = 0;
  for share in settings.shares.iter().filter(|s| !s.excluded) {
    if let Err(e) = std::fs::metadata(&share.local_path).and_then(|m| {
      if m.is_dir() {
        Ok(())
      } else {
        Err(std::io::Error::other("not a directory"))
      }
    }) {
      warn!(share = %share.local_path.display(), error = %e, "Failed to scan share");
      continue;
    }
    let (found, out) = directories_under(settings, &share.local_path, true, cancel, &mut seen);
    all.extend(found);
    left_out += out;
  }
  (all, left_out)
}

/// The files directly in a directory, read from disk.
#[derive(Debug)]
pub(super) struct Read {
  /// the directory and its files as the index writes them
  pub rows: DirectoryRows,
  /// files a filter left out
  pub filtered: u32,
  /// files whose name was not UTF-8
  pub lossy: u32,
}

/// Reads the files directly in `directory`, which `share` owns: their size, modification time
/// and attributes, reusing those of `known` rows that did not change unless `force`. One file
/// that cannot be read is logged and left out; its siblings are read.
pub(super) fn read_directory(
  settings: &Settings,
  share: &Share,
  directory: &Path,
  known: &HashMap<String, Known>,
  force: bool,
) -> Option<Read> {
  let masked = share::mask(share, directory)?;
  let mut read = Read {
    rows: DirectoryRows {
      name: masked.name,
      files: Vec::new(),
    },
    filtered: 0,
    lossy: u32::from(masked.lossy),
  };
  let entries = match std::fs::read_dir(directory) {
    Ok(entries) => entries,
    Err(e) => {
      warn!(directory = %directory.display(), error = %e, "Failed to scan files in directory");
      return Some(read);
    }
  };
  for entry in entries {
    let entry = match entry {
      Ok(entry) => entry,
      Err(e) => {
        warn!(directory = %directory.display(), error = %e, "Failed to scan files in directory");
        continue;
      }
    };
    let path = entry.path();
    let link = entry.file_type().is_ok_and(|t| t.is_symlink());
    if link && !settings.follow_symlinks {
      continue;
    }
    let metadata = match std::fs::metadata(&path) {
      Ok(metadata) => metadata,
      Err(e) => {
        debug!(file = %path.display(), error = %e, "file not read");
        continue;
      }
    };
    if !metadata.is_file() || entries::left_out(&entry.file_name(), Some(&metadata)) {
      continue;
    }
    if settings.filtered(&path) {
      read.filtered += 1;
      continue;
    }
    let Some(file) = share::mask(share, &path) else {
      continue;
    };
    read.lossy += u32::from(file.lossy);
    let touched_at = metadata
      .modified()
      .ok()
      .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
      .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    let name = entry.file_name();
    let extension = attributes::extension(&name.to_string_lossy());
    let size = metadata.len();
    let attributes = match known.get(&file.name) {
      Some(row) if !force && row.size == size && row.touched_at == touched_at => {
        row.attributes.clone()
      }
      _ => attributes::read(&path, &extension),
    };
    read.rows.files.push(FileRow {
      masked: file.name,
      original: repository::path_bytes(&path),
      size,
      touched_at,
      extension,
      attributes,
    });
  }
  Some(read)
}

/// Reads one directory with its known rows from the index, on a blocking thread.
pub(super) async fn read_from_disk(
  db: &Database,
  settings: &Arc<Settings>,
  directory: PathBuf,
  force: bool,
) -> Result<Option<Read>, ScanError> {
  let Some(share) = share::owner(&settings.shares, &directory).cloned() else {
    return Ok(None);
  };
  let Some(masked) = share::mask(&share, &directory) else {
    return Ok(None);
  };
  let known = db
    .read(move |conn| repository::known_files(conn, &masked.name))
    .await?;
  let settings = settings.clone();
  tokio::task::spawn_blocking(move || {
    debug!(directory = %directory.display(), "Starting scan of directory");
    read_directory(&settings, &share, &directory, &known, force)
  })
  .await
  .map_err(|e| ScanError::Task(e.to_string()))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::shares::scanner::tests::settings;

  #[cfg(unix)]
  #[test]
  fn a_directory_reached_twice_through_links_is_walked_once() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir_all(music.join("A")).unwrap();
    std::fs::create_dir_all(music.join("B")).unwrap();
    std::os::unix::fs::symlink(music.join("A"), music.join("B/link")).unwrap();
    std::os::unix::fs::symlink(&music, music.join("A/loop")).unwrap();
    let followed = settings(dir.path(), &[], &[], true);
    let (found, _) = directories_under(
      &followed,
      &music,
      true,
      &CancellationToken::new(),
      &mut HashSet::new(),
    );
    let names: Vec<_> = found
      .iter()
      .map(|p| p.strip_prefix(&music).unwrap().display().to_string())
      .collect();
    assert_eq!(names, ["", "A", "B"], "B/link is A again, A/loop the root");
  }

  #[cfg(unix)]
  #[test]
  fn a_directory_that_can_not_be_read_is_left_out_with_one_warning() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir_all(music.join("open")).unwrap();
    std::fs::create_dir_all(music.join("shut/inner")).unwrap();
    std::fs::set_permissions(music.join("shut"), std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(music.join("shut")).is_ok() {
      // run with privileges that read any directory, there is nothing to leave out
      return;
    }
    let settings = settings(dir.path(), &[], &[], false);
    let ((found, _), lines) = crate::testing::logged(|| {
      directories_under(
        &settings,
        &music,
        true,
        &CancellationToken::new(),
        &mut HashSet::new(),
      )
    });
    std::fs::set_permissions(music.join("shut"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let names: Vec<_> = found
      .iter()
      .map(|p| p.strip_prefix(&music).unwrap().display().to_string())
      .collect();
    assert_eq!(names, ["", "open"]);
    assert_eq!(lines.matches("cannot be read").count(), 1, "{lines}");
  }
}
