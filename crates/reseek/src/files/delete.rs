//! Deleting a file or a directory under a root. On Unix the path is walked from the opened root
//! one directory at a time without following links, and the last name is removed from the
//! directory it is in, so what was checked is what is removed; a directory goes with everything
//! in it, its links removed as links.

use std::io;
use std::path::Path;

use tracing::{debug, warn};

use super::roots::{Refused, locate};
use super::writing::Writing;
use super::{named, unreadable};
use crate::api::error::ApiError;

/// What a deletion removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
  /// one file, or one link
  File,
  /// a directory and everything in it, or one link
  Directory,
}

impl Kind {
  fn noun(self) -> &'static str {
    match self {
      Kind::File => "file",
      Kind::Directory => "directory",
    }
  }
}

/// Deletes `relative` under `root`; a file being written by a download is a conflict.
pub(super) fn delete(
  root: &Path,
  relative: &str,
  kind: Kind,
  writing: &Writing,
) -> Result<(), ApiError> {
  let shown = root.join(relative);
  let noun = kind.noun();
  let missing = || ApiError::not_found(format!("The {noun} {} does not exist", named(relative)));
  let outside = || {
    warn!(path = %shown.display(), kind = noun, "deletion outside the application's directories refused");
    let message = match kind {
      Kind::File => "Only files in application-controlled directories can be deleted",
      Kind::Directory => "Only application-controlled directories can be deleted",
    };
    ApiError::forbidden(message).logged()
  };
  let located = match locate(root, relative) {
    Ok(located) => located,
    Err(Refused::RootMissing) => return Err(missing()),
    Err(Refused::Outside) => return Err(outside()),
    Err(Refused::Denied(e)) => return Err(denied(relative, noun, &e)),
    Err(Refused::Io(e)) => return Err(unreadable(noun, relative, &shown, e)),
  };
  let segments: Vec<&str> = relative.split('/').filter(|s| !s.is_empty()).collect();
  let Some((name, parents)) = segments.split_last() else {
    warn!(path = %shown.display(), "deletion of a directory root refused");
    return Err(
      ApiError::forbidden("Deletion of application-controlled directory roots is not supported")
        .logged(),
    );
  };
  if writing.holds(&[&located.path, &located.resolved]) {
    return Err(ApiError::conflict(
      format!("{} is being written by a download", named(relative)),
      None,
    ));
  }
  debug!(path = %shown.display(), kind = noun, "deleting");
  match remove(&located.root_resolved, parents, name, kind) {
    Ok(()) => Ok(()),
    Err(Failure::Missing) => Err(missing()),
    Err(Failure::Link) => {
      warn!(path = %shown.display(), "deletion through a link refused");
      Err(ApiError::forbidden("Paths that lead through a link cannot be deleted").logged())
    }
    Err(Failure::Io(e)) if e.kind() == io::ErrorKind::PermissionDenied => {
      warn!(path = %shown.display(), error = %e, "deletion denied by the system");
      Err(denied(relative, noun, &e).logged())
    }
    Err(Failure::Io(e)) => Err(ApiError::internal(format!(
      "{noun} {} not deleted: {e}",
      shown.display()
    ))),
  }
}

fn denied(relative: &str, noun: &str, error: &io::Error) -> ApiError {
  ApiError::forbidden(format!(
    "Deletion of the {noun} {} was denied: {error}",
    named(relative)
  ))
}

/// Why a removal did not happen.
#[derive(Debug)]
enum Failure {
  /// a name on the way, or the last one, is not there or not what was asked for
  Missing,
  /// a directory on the way is a link
  Link,
  /// anything else the system said
  Io(io::Error),
}

#[cfg(unix)]
fn remove(root: &Path, parents: &[&str], name: &str, kind: Kind) -> Result<(), Failure> {
  use rustix::fs::{AtFlags, FileType, Mode, OFlags, open, openat, statat, unlinkat};
  use rustix::io::Errno;

  let directory = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
  let io_failure = |e: Errno| Failure::Io(io::Error::from(e));
  let mut dir = open(root, directory, Mode::empty()).map_err(io_failure)?;
  for parent in parents {
    dir = match openat(&dir, *parent, directory, Mode::empty()) {
      Ok(next) => next,
      Err(Errno::LOOP) => return Err(Failure::Link),
      Err(Errno::NOENT) => return Err(Failure::Missing),
      Err(Errno::NOTDIR) => {
        let kind = statat(&dir, *parent, AtFlags::SYMLINK_NOFOLLOW)
          .map(|s| FileType::from_raw_mode(s.st_mode));
        return Err(match kind {
          Ok(FileType::Symlink) => Failure::Link,
          _ => Failure::Missing,
        });
      }
      Err(e) => return Err(io_failure(e)),
    };
  }
  let found = match statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
    Ok(stat) => FileType::from_raw_mode(stat.st_mode),
    Err(Errno::NOENT) => return Err(Failure::Missing),
    Err(e) => return Err(io_failure(e)),
  };
  match (kind, found) {
    // a link goes as a link, whatever it points at
    (_, FileType::Symlink) => unlinkat(&dir, name, AtFlags::empty()).map_err(io_failure),
    (Kind::File, FileType::Directory) | (Kind::Directory, FileType::RegularFile) => {
      Err(Failure::Missing)
    }
    (Kind::File, _) => unlinkat(&dir, name, AtFlags::empty()).map_err(io_failure),
    (Kind::Directory, _) => std::ffi::CString::new(name)
      // a path from the API never holds a NUL, which the decoder refuses
      .map_err(|_| io_failure(rustix::io::Errno::INVAL))
      .and_then(|name| remove_tree(&dir, &name).map_err(io_failure)),
  }
}

/// Removes the directory `name` in `parent` with everything in it, never following a link.
#[cfg(unix)]
fn remove_tree(parent: &rustix::fd::OwnedFd, name: &std::ffi::CStr) -> rustix::io::Result<()> {
  use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, openat, statat, unlinkat};

  let fd = openat(
    parent,
    name,
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
    Mode::empty(),
  )?;
  let mut names = Vec::new();
  for entry in Dir::read_from(&fd)? {
    let entry = entry?;
    let entry_name = entry.file_name().to_bytes();
    if entry_name != b"." && entry_name != b".." {
      names.push(entry.file_name().to_owned());
    }
  }
  for entry in names {
    let stat = statat(&fd, entry.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
    if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
      remove_tree(&fd, &entry)?;
    } else {
      unlinkat(&fd, entry.as_c_str(), AtFlags::empty())?;
    }
  }
  unlinkat(parent, name, AtFlags::REMOVEDIR)
}

/// Removes by path where the system offers no removal relative to an opened directory; the
/// walk's checks are made just before.
#[cfg(not(unix))]
fn remove(root: &Path, parents: &[&str], name: &str, kind: Kind) -> Result<(), Failure> {
  let mut path = root.to_path_buf();
  for parent in parents {
    path.push(parent);
    match std::fs::symlink_metadata(&path) {
      Ok(m) if m.file_type().is_symlink() => return Err(Failure::Link),
      Ok(m) if m.is_dir() => {}
      Ok(_) => return Err(Failure::Missing),
      Err(e) if super::roots::missing(&e) => return Err(Failure::Missing),
      Err(e) => return Err(Failure::Io(e)),
    }
  }
  path.push(name);
  let found = match std::fs::symlink_metadata(&path) {
    Ok(found) => found,
    Err(e) if super::roots::missing(&e) => return Err(Failure::Missing),
    Err(e) => return Err(Failure::Io(e)),
  };
  let result = if found.file_type().is_symlink() {
    std::fs::remove_file(&path).or_else(|_| std::fs::remove_dir(&path))
  } else {
    match (kind, found.is_dir()) {
      (Kind::File, false) => std::fs::remove_file(&path),
      (Kind::Directory, true) => std::fs::remove_dir_all(&path),
      _ => return Err(Failure::Missing),
    }
  };
  result.map_err(Failure::Io)
}

#[cfg(test)]
mod tests {
  use super::*;

  struct Tree {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    outside: std::path::PathBuf,
  }

  fn tree() -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("downloads");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(root.join("album/discs")).unwrap();
    std::fs::write(root.join("album/01.flac"), b"x").unwrap();
    std::fs::write(root.join("album/discs/02.flac"), b"x").unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("passwd"), b"x").unwrap();
    Tree {
      _dir: dir,
      root,
      outside,
    }
  }

  #[test]
  fn a_directory_is_no_file_and_the_root_is_never_deleted() {
    let t = tree();
    let writing = Writing::default();
    assert_eq!(
      delete(&t.root, "album", Kind::File, &writing)
        .unwrap_err()
        .status(),
      404,
      "a directory is no file"
    );
    assert_eq!(
      delete(&t.root, "", Kind::Directory, &writing)
        .unwrap_err()
        .status(),
      403,
      "never the root"
    );
  }

  #[cfg(target_os = "linux")]
  #[test]
  fn a_tree_with_a_name_that_is_not_utf8_is_deleted_whole() {
    use std::os::unix::ffi::OsStrExt;
    let t = tree();
    let odd = std::ffi::OsStr::from_bytes(b"caf\xe9");
    std::fs::create_dir_all(t.root.join("album").join(odd)).unwrap();
    std::fs::write(t.root.join("album").join(odd).join("x"), b"x").unwrap();
    delete(&t.root, "album", Kind::Directory, &Writing::default()).unwrap();
    assert!(!t.root.join("album").exists());
  }

  #[test]
  fn a_file_being_written_is_not_deleted() {
    let t = tree();
    let writing = Writing::default();
    let guard = writing
      .hold(vec![t.root.join("album/discs/02.flac")])
      .unwrap();
    for (path, kind) in [
      ("album/discs/02.flac", Kind::File),
      ("album", Kind::Directory),
    ] {
      let refused = delete(&t.root, path, kind, &writing).unwrap_err();
      assert_eq!(refused.status(), 409, "{path}");
    }
    drop(guard);
    delete(&t.root, "album", Kind::Directory, &writing).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn links_are_never_followed_out_of_the_root() {
    use std::os::unix::fs::symlink;
    let t = tree();
    let writing = Writing::default();
    symlink(&t.outside, t.root.join("out")).unwrap();
    symlink(t.root.join("album"), t.root.join("in")).unwrap();
    symlink(t.outside.join("passwd"), t.root.join("album/passwd")).unwrap();
    let through = delete(&t.root, "out/passwd", Kind::File, &writing).unwrap_err();
    assert_eq!(through.status(), 403);
    assert!(t.outside.join("passwd").exists());
    let inside = delete(&t.root, "in/01.flac", Kind::File, &writing).unwrap_err();
    assert_eq!(
      inside.status(),
      403,
      "no path through a link, even one inside"
    );
    // a link inside a deleted directory goes as a link, its target stays
    std::fs::create_dir_all(t.root.join("album/discs/deeper")).unwrap();
    symlink(&t.outside, t.root.join("album/discs/deeper/away")).unwrap();
    delete(&t.root, "album", Kind::Directory, &writing).unwrap();
    assert!(t.outside.join("passwd").exists());
    assert!(!t.root.join("album").exists());
    // the link itself is removable
    delete(&t.root, "in", Kind::Directory, &writing).unwrap();
    assert!(std::fs::symlink_metadata(t.root.join("in")).is_err());
  }
}
