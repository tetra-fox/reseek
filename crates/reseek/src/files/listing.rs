//! Listing a directory under a root: its directories, then its files, by name, or with
//! `recursive` everything under it in two flat lists. Links are listed and never entered;
//! hidden entries are listed; a subdirectory that cannot be read is left out.

use std::fs::Metadata;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use api_types::files::{FilesystemAttributes, FilesystemDirectory, FilesystemFile};
use chrono::{DateTime, Utc};
use tracing::{debug, warn};

use super::roots::{Refused, locate};
use super::{named, unreadable};
use crate::api::error::ApiError;
use crate::entries;

/// One entry of a directory, read without following it.
pub(super) enum Entry {
  /// a directory, or a link to one
  Directory(FilesystemDirectory),
  /// anything else
  File(FilesystemFile),
}

/// The directory `relative` under `root` with its entries.
pub(super) fn list(
  root: &Path,
  relative: &str,
  recursive: bool,
) -> Result<FilesystemDirectory, ApiError> {
  let shown = root.join(relative);
  let missing = || ApiError::not_found(format!("The directory {} does not exist", named(relative)));
  let located = match locate(root, relative) {
    Ok(located) => located,
    Err(Refused::RootMissing) => return Err(missing()),
    Err(Refused::Outside) => {
      warn!(path = %shown.display(), "listing outside the application's directories refused");
      return Err(
        ApiError::forbidden("Only application-controlled directories can be listed").logged(),
      );
    }
    Err(Refused::Denied(e) | Refused::Io(e)) => {
      return Err(unreadable("directory", relative, &shown, e));
    }
  };
  if !located.exists {
    return Err(missing());
  }
  let metadata = std::fs::metadata(&located.resolved)
    .map_err(|e| unreadable("directory", relative, &shown, e))?;
  if !metadata.is_dir() {
    return Err(missing());
  }
  let link = std::fs::symlink_metadata(&located.path).is_ok_and(|m| m.file_type().is_symlink());
  let name = located.name();
  let mut top = directory(&name, String::new(), link, &metadata);
  let mut files = Vec::new();
  let mut directories = Vec::new();
  let entries =
    read(&located.resolved).map_err(|e| unreadable("directory", relative, &shown, e))?;
  collect(
    &located.resolved,
    "",
    entries,
    recursive,
    &mut directories,
    &mut files,
  );
  directories.sort_by(|a, b| a.full_name.cmp(&b.full_name));
  files.sort_by(|a, b| a.full_name.cmp(&b.full_name));
  top.files = Some(files);
  top.directories = Some(directories);
  Ok(top)
}

/// Adds the entries of `dir`, whose path relative to the listed directory is `prefix`, and
/// with `recursive` those of every real subdirectory.
fn collect(
  dir: &Path,
  prefix: &str,
  entries: Vec<(String, Entry)>,
  recursive: bool,
  directories: &mut Vec<FilesystemDirectory>,
  files: &mut Vec<FilesystemFile>,
) {
  for (name, entry) in entries {
    match entry {
      Entry::Directory(listed) => {
        let enter = recursive
          && !listed
            .attributes
            .contains(FilesystemAttributes::ReparsePoint);
        let full_name = listed.full_name.clone();
        directories.push(relative_to(listed, prefix));
        if enter {
          let path = dir.join(&name);
          match read(&path) {
            Ok(inner) => {
              let inner_prefix = joined(prefix, &full_name);
              collect(&path, &inner_prefix, inner, recursive, directories, files);
            }
            Err(e) => {
              debug!(path = %path.display(), error = %e, "unreadable directory left out of a listing")
            }
          }
        }
      }
      Entry::File(file) => files.push(file_relative_to(file, prefix)),
    }
  }
}

/// `name` below the relative path `prefix`, with the system's separator.
fn joined(prefix: &str, name: &str) -> String {
  if prefix.is_empty() {
    name.to_string()
  } else {
    format!("{prefix}{}{name}", std::path::MAIN_SEPARATOR)
  }
}

fn relative_to(mut directory: FilesystemDirectory, prefix: &str) -> FilesystemDirectory {
  directory.full_name = joined(prefix, &directory.full_name);
  directory
}

fn file_relative_to(mut file: FilesystemFile, prefix: &str) -> FilesystemFile {
  file.full_name = joined(prefix, &file.full_name);
  file
}

/// The entries of `dir`, each named by itself and read without following links.
pub(super) fn read(dir: &Path) -> io::Result<Vec<(String, Entry)>> {
  let mut entries = Vec::new();
  for entry in std::fs::read_dir(dir)? {
    let entry = match entry {
      Ok(entry) => entry,
      Err(e) => {
        debug!(dir = %dir.display(), error = %e, "unreadable entry left out of a listing");
        continue;
      }
    };
    let name = entry.file_name().to_string_lossy().into_owned();
    let path = entry.path();
    let own = match std::fs::symlink_metadata(&path) {
      Ok(own) => own,
      Err(e) => {
        debug!(path = %path.display(), error = %e, "entry that cannot be read left out of a listing");
        continue;
      }
    };
    if entries::system(&own) {
      continue;
    }
    let link = own.file_type().is_symlink();
    // a link is shown as what it points at, or as a file when it points at nothing
    let shown = if link {
      std::fs::metadata(&path).unwrap_or(own)
    } else {
      own
    };
    let listed = if shown.is_dir() {
      Entry::Directory(directory(&name, name.clone(), link, &shown))
    } else {
      Entry::File(file(&name, name.clone(), link, &shown))
    };
    entries.push((name, listed));
  }
  Ok(entries)
}

/// A directory entry as a listing shows it.
fn directory(
  name: &str,
  full_name: String,
  link: bool,
  metadata: &Metadata,
) -> FilesystemDirectory {
  FilesystemDirectory {
    name: name.to_string(),
    full_name,
    attributes: attributes(name, link, metadata),
    created_at: created(metadata),
    modified_at: time(metadata.modified().ok()),
    files: None,
    directories: None,
  }
}

/// A file entry as a listing shows it; a link to nothing has no length.
pub(super) fn file(
  name: &str,
  full_name: String,
  link: bool,
  metadata: &Metadata,
) -> FilesystemFile {
  let length = if metadata.file_type().is_symlink() {
    0
  } else {
    metadata.len()
  };
  FilesystemFile {
    name: name.to_string(),
    full_name,
    length,
    attributes: attributes(name, link, metadata),
    created_at: created(metadata),
    modified_at: time(metadata.modified().ok()),
  }
}

/// What kind of entry it is, in the names Windows gives its file attributes.
#[cfg(not(windows))]
pub(super) fn attributes(name: &str, link: bool, metadata: &Metadata) -> FilesystemAttributes {
  use std::os::unix::fs::PermissionsExt;
  let mut attributes = FilesystemAttributes::empty();
  if metadata.is_dir() {
    attributes |= FilesystemAttributes::Directory;
  }
  if link {
    attributes |= FilesystemAttributes::ReparsePoint;
  }
  if entries::hidden(std::ffi::OsStr::new(name), Some(metadata)) {
    attributes |= FilesystemAttributes::Hidden;
  }
  if metadata.permissions().mode() & 0o200 == 0 {
    attributes |= FilesystemAttributes::ReadOnly;
  }
  if attributes.is_empty() {
    attributes = FilesystemAttributes::Normal;
  }
  attributes
}

/// What kind of entry it is, as Windows marks it.
#[cfg(windows)]
pub(super) fn attributes(_name: &str, link: bool, metadata: &Metadata) -> FilesystemAttributes {
  use std::os::windows::fs::MetadataExt;
  let mut attributes = FilesystemAttributes::from_bits_truncate(metadata.file_attributes());
  if link {
    attributes |= FilesystemAttributes::ReparsePoint;
  }
  attributes
}

/// When the entry was created, or when its metadata last changed where the system keeps no
/// creation time.
fn created(metadata: &Metadata) -> DateTime<Utc> {
  if let Ok(created) = metadata.created() {
    return time(Some(created));
  }
  #[cfg(unix)]
  {
    use std::os::unix::fs::MetadataExt;
    if let Some(changed) = DateTime::from_timestamp(metadata.ctime(), metadata.ctime_nsec() as u32)
    {
      return changed;
    }
  }
  time(metadata.modified().ok())
}

/// A system time as a UTC time, the epoch when the system gives none.
pub(super) fn time(at: Option<SystemTime>) -> DateTime<Utc> {
  at.map(DateTime::<Utc>::from).unwrap_or_default()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn listings_put_directories_first_by_name_and_show_links_without_entering_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("downloads");
    std::fs::create_dir_all(root.join("b album/discs")).unwrap();
    std::fs::create_dir_all(root.join("a album")).unwrap();
    std::fs::write(root.join("b album/01.flac"), b"12345").unwrap();
    std::fs::write(root.join("b album/discs/x.flac"), b"1").unwrap();
    std::fs::write(root.join(".hidden"), b"").unwrap();
    std::fs::write(root.join("z.txt"), b"").unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret"), b"").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

    let top = list(&root, "", false).unwrap();
    assert_eq!(top.full_name, "");
    let names =
      |list: &[FilesystemDirectory]| list.iter().map(|d| d.name.clone()).collect::<Vec<_>>();
    let directories = top.directories.unwrap();
    let files: Vec<_> = top.files.unwrap().into_iter().map(|f| f.name).collect();
    assert_eq!(files, [".hidden", "z.txt"]);
    #[cfg(unix)]
    {
      assert_eq!(names(&directories), ["a album", "b album", "link"]);
      let link = &directories[2];
      assert_eq!(
        link.attributes,
        FilesystemAttributes::Directory | FilesystemAttributes::ReparsePoint
      );
    }

    let all = list(&root, "", true).unwrap();
    let files: Vec<_> = all
      .files
      .unwrap()
      .into_iter()
      .map(|f| f.full_name)
      .collect();
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(
      files,
      [
        ".hidden".to_string(),
        format!("b album{sep}01.flac"),
        format!("b album{sep}discs{sep}x.flac"),
        "z.txt".to_string()
      ],
      "nothing from behind the link"
    );
    let inner = list(&root, "b album", false).unwrap();
    assert_eq!(inner.name, "b album");
    assert_eq!(inner.files.unwrap()[0].length, 5);
    assert_eq!(list(&root, "nothing", false).unwrap_err().status(), 404);
    assert_eq!(list(&root, "z.txt", false).unwrap_err().status(), 404);
    #[cfg(unix)]
    assert_eq!(list(&root, "link", false).unwrap_err().status(), 403);
  }
}
