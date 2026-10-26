//! Finding a path under one of the directories the files service may touch, and refusing one
//! that leads out of it. A path is joined to its root as written, then resolved through every
//! link it passes: the resolved path must still be under the resolved root.

use std::io;
use std::path::{Path, PathBuf};

/// A path under a root, found.
#[derive(Debug)]
pub(super) struct Located {
  /// the root resolved through links
  pub root_resolved: PathBuf,
  /// the root joined with the path as asked for
  pub path: PathBuf,
  /// `path` resolved through links; for a path that does not exist, its longest existing
  /// part resolved with the rest after it
  pub resolved: PathBuf,
  /// whether `path` exists
  pub exists: bool,
}

/// Why a path could not be used.
#[derive(Debug)]
pub(super) enum Refused {
  /// the root directory itself does not exist
  RootMissing,
  /// the path leads out of its root through a link
  Outside,
  /// the system denied reading on the way
  Denied(io::Error),
  /// anything else the system said
  Io(io::Error),
}

impl Located {
  /// The path's name, the root's own name for the root; empty for a filesystem root, which
  /// has none.
  pub fn name(&self) -> String {
    self
      .path
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
      .unwrap_or_default()
  }
}

/// `relative`, a `/`-separated path without `.`, `..` or drive-relative segments, found under
/// `root`.
pub(super) fn locate(root: &Path, relative: &str) -> Result<Located, Refused> {
  let root_resolved = std::fs::canonicalize(root).map_err(|e| match e.kind() {
    io::ErrorKind::NotFound => Refused::RootMissing,
    io::ErrorKind::PermissionDenied => Refused::Denied(e),
    _ => Refused::Io(e),
  })?;
  let mut path = root.to_path_buf();
  for segment in relative.split('/').filter(|s| !s.is_empty()) {
    path.push(segment);
  }
  let (resolved, exists) = match std::fs::canonicalize(&path) {
    Ok(resolved) => (resolved, true),
    Err(e) if missing(&e) => (resolve_existing_part(&path)?, false),
    Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(Refused::Denied(e)),
    Err(e) => return Err(Refused::Io(e)),
  };
  if !inside(&resolved, &root_resolved) {
    return Err(Refused::Outside);
  }
  Ok(Located {
    root_resolved,
    path,
    resolved,
    exists,
  })
}

/// The entry a directory read gave, or `None` with a debug line when the system refused it;
/// `walk` names what is reading, such as "archive".
pub(super) fn entry_of(
  entry: io::Result<std::fs::DirEntry>,
  dir: &Path,
  walk: &str,
) -> Option<std::fs::DirEntry> {
  entry
    .inspect_err(|e| tracing::debug!(dir = %dir.display(), error = %e, walk, "entry not read"))
    .ok()
}

/// The metadata of `path` itself, not of a link's target, or `None` with a debug line when
/// the system refused it.
pub(super) fn metadata_of(path: &Path, walk: &str) -> Option<std::fs::Metadata> {
  std::fs::symlink_metadata(path)
    .inspect_err(|e| tracing::debug!(path = %path.display(), error = %e, walk, "entry not read"))
    .ok()
}

/// Whether an error says a path, or a directory on its way, is not there.
pub(super) fn missing(error: &io::Error) -> bool {
  matches!(
    error.kind(),
    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
  )
}

/// `path` with its longest existing part resolved and the rest after it as written.
fn resolve_existing_part(path: &Path) -> Result<PathBuf, Refused> {
  for ancestor in path.ancestors().skip(1) {
    match std::fs::canonicalize(ancestor) {
      Ok(resolved) => {
        let rest = path
          .strip_prefix(ancestor)
          .expect("an ancestor is a prefix of its path");
        return Ok(resolved.join(rest));
      }
      Err(e) if missing(&e) => continue,
      Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(Refused::Denied(e)),
      Err(e) => return Err(Refused::Io(e)),
    }
  }
  Ok(path.to_path_buf())
}

/// Whether `path` is `root` or under it, segment by segment, ignoring case where the usual
/// filesystems do: on Windows and macOS.
pub(super) fn inside(path: &Path, root: &Path) -> bool {
  if !cfg!(any(windows, target_os = "macos")) {
    return path.starts_with(root);
  }
  let fold = |p: &Path| {
    p.components()
      .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
      .collect::<Vec<_>>()
  };
  let (path, root) = (fold(path), fold(root));
  path.len() >= root.len() && path[..root.len()] == root[..]
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn paths_stay_under_their_root_through_links() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("downloads");
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(root.join("album")).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    let found = locate(&root, "album").unwrap();
    assert!(found.exists && found.resolved != found.root_resolved);
    assert_eq!(found.name(), "album");
    let top = locate(&root, "").unwrap();
    assert_eq!(top.resolved, top.root_resolved);
    let missing = locate(&root, "album/new/deeper").unwrap();
    assert!(!missing.exists);
    assert!(missing.resolved.ends_with("album/new/deeper"));
    assert!(matches!(
      locate(&dir.path().join("none"), "x"),
      Err(Refused::RootMissing)
    ));
    #[cfg(unix)]
    {
      std::os::unix::fs::symlink(&elsewhere, root.join("out")).unwrap();
      std::os::unix::fs::symlink(root.join("album"), root.join("in")).unwrap();
      assert!(matches!(locate(&root, "out"), Err(Refused::Outside)));
      assert!(matches!(locate(&root, "out/x"), Err(Refused::Outside)));
      assert!(locate(&root, "in").unwrap().exists, "a link inside is fine");
    }
  }

  #[test]
  fn inside_compares_whole_segments() {
    assert!(inside(Path::new("/a/b/c"), Path::new("/a/b")));
    assert!(inside(Path::new("/a/b"), Path::new("/a/b")));
    assert!(!inside(Path::new("/a/bc"), Path::new("/a/b")));
  }
}
