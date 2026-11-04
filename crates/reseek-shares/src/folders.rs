//! The folders of this machine, for picking a directory to share. Only folders are named, and
//! only ones a scan of the shares would walk into: hidden and system folders, links unless they
//! are followed, excluded and filtered folders and folders that cannot be read are neither
//! listed nor listed from. All of those answer as a path that does not exist does, so the answer
//! tells nothing about what is there, files above all.

use std::ffi::OsStr;
use std::fs::{self, DirEntry, Metadata};
use std::path::{Component, Path, PathBuf};

use api_types::shares::{ShareFolder, ShareFolders};
use tracing::debug;

use super::scanner::Settings;
use super::share;
use crate::entries;

/// The most folders listed from one folder.
const MAX_FOLDERS: usize = 5000;

/// Why no folders were listed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FolderError {
  /// the path names no folder a share could be made of, or one that cannot be read
  #[error("No folder that can be shared is there")]
  NotListed,
  /// the path is relative
  #[error("The path must be absolute")]
  Relative,
  /// the path holds `.` or `..`
  #[error("The path may not hold . or ..")]
  Traverses,
  /// the listing did not finish
  #[error("the folders were not listed: {0}")]
  Failed(String),
}

/// Whether a scan leaves out the folder at `path`, named `name`.
fn left_out(settings: &Settings, path: &Path, name: &OsStr, metadata: &Metadata) -> bool {
  entries::left_out(name, Some(metadata))
    || share::excluded(&settings.shares, path)
    || settings.filtered(path)
}

/// Whether every step down to `path` is one a scan takes. The steps are looked at one by one:
/// a share's walk reaches a folder only through the folders above it.
fn reachable(settings: &Settings, path: &Path) -> Result<(), FolderError> {
  let mut at = PathBuf::new();
  for part in path.components() {
    at.push(part);
    let name = match part {
      Component::Prefix(_) | Component::RootDir => continue,
      Component::Normal(name) => name,
      Component::CurDir | Component::ParentDir => {
        return Err(FolderError::Traverses);
      }
    };
    let unseen = |e: std::io::Error| {
      debug!(path = %at.display(), error = %e, "folder not listed");
      FolderError::NotListed
    };
    let link = fs::symlink_metadata(&at).map_err(unseen)?.is_symlink();
    let metadata = fs::metadata(&at).map_err(unseen)?;
    let followed = !link || settings.follow_symlinks;
    if !metadata.is_dir() || !followed || left_out(settings, &at, name, &metadata) {
      return Err(FolderError::NotListed);
    }
  }
  Ok(())
}

/// `entry` as a choice, when it is a folder a scan would walk into. What cannot be looked at is
/// left out as a scan leaves it out, and so is a name a share entry cannot hold, which has to
/// be text.
fn choice(settings: &Settings, entry: &DirEntry) -> Option<ShareFolder> {
  let kind = entry.file_type().ok()?;
  if !kind.is_dir() && !(kind.is_symlink() && settings.follow_symlinks) {
    return None;
  }
  let path = entry.path();
  let metadata = fs::metadata(&path).ok()?;
  if !metadata.is_dir() || left_out(settings, &path, &entry.file_name(), &metadata) {
    return None;
  }
  fs::read_dir(&path).ok()?;
  Some(ShareFolder {
    name: entry.file_name().into_string().ok()?,
    path: path.into_os_string().into_string().ok()?,
  })
}

/// The folders directly in `path`, which is absolute.
fn under(settings: &Settings, path: &str) -> Result<ShareFolders, FolderError> {
  reachable(settings, Path::new(path))?;
  let entries = fs::read_dir(path).map_err(|e| {
    debug!(path, error = %e, "folder not listed");
    FolderError::NotListed
  })?;
  let mut folders = Vec::new();
  let mut truncated = false;
  for entry in entries {
    let entry = match entry {
      Ok(entry) => entry,
      Err(e) => {
        debug!(path, error = %e, "a folder entry could not be read");
        continue;
      }
    };
    let Some(folder) = choice(settings, &entry) else {
      continue;
    };
    if folders.len() == MAX_FOLDERS {
      truncated = true;
      break;
    }
    folders.push(folder);
  }
  folders.sort_by_cached_key(|folder| folder.name.to_lowercase());
  Ok(ShareFolders {
    path: Some(path.to_string()),
    folders,
    truncated,
  })
}

/// The top of the machine: every drive.
#[cfg(windows)]
fn top(_settings: &Settings) -> Result<ShareFolders, FolderError> {
  let folders = ('A'..='Z')
    .map(|letter| ShareFolder {
      name: format!("{letter}:"),
      path: format!("{letter}:\\"),
    })
    .filter(|drive| Path::new(&drive.path).is_dir())
    .collect();
  Ok(ShareFolders {
    path: None,
    folders,
    truncated: false,
  })
}

/// The top of the machine: what is in `/`.
#[cfg(not(windows))]
fn top(settings: &Settings) -> Result<ShareFolders, FolderError> {
  under(settings, "/")
}

/// The folders in `path`, or at the top of the machine without one.
pub(super) fn list(settings: &Settings, path: Option<&str>) -> Result<ShareFolders, FolderError> {
  match path {
    None => top(settings),
    Some(path) if Path::new(path).is_absolute() => under(settings, path),
    Some(_) => Err(FolderError::Relative),
  }
}
