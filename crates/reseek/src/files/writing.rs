//! The files downloads are writing now. Such a file is not served or deleted, and pruning
//! leaves it alone, until the download lets go of it. A file has one writer at a time.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use reseek_shares::lock;

use super::roots::inside;

/// The paths being written, each by every spelling it was marked with, shared by every handle.
#[derive(Clone, Default)]
pub struct Writing {
  paths: Arc<Mutex<Vec<PathBuf>>>,
}

/// Marks one file as being written until it is dropped.
#[must_use = "the file counts as written only while the guard lives"]
pub struct WriteGuard {
  paths: Arc<Mutex<Vec<PathBuf>>>,
  held: Vec<PathBuf>,
}

impl Drop for WriteGuard {
  fn drop(&mut self) {
    let mut paths = lock(&self.paths);
    for path in &self.held {
      if let Some(at) = paths.iter().position(|p| p == path) {
        paths.swap_remove(at);
      }
    }
  }
}

impl Writing {
  /// Marks one file as being written, by each of `spellings`: the path as written under the
  /// configured directory and the same path resolved through links. Fails with `ResourceBusy`
  /// when the file is held already by any of them, letter case ignored where the usual
  /// filesystems ignore it, since two names that differ only so are one file there.
  pub fn hold(&self, spellings: Vec<PathBuf>) -> io::Result<WriteGuard> {
    let mut paths = lock(&self.paths);
    let same = |a: &Path, b: &Path| inside(a, b) && inside(b, a);
    if let Some(taken) = spellings
      .iter()
      .find(|spelling| paths.iter().any(|held| same(held, spelling)))
    {
      return Err(io::Error::new(
        io::ErrorKind::ResourceBusy,
        format!("another download is writing {}", taken.display()),
      ));
    }
    paths.extend(spellings.iter().cloned());
    Ok(WriteGuard {
      paths: self.paths.clone(),
      held: spellings,
    })
  }

  /// Whether a file being written is `path` or under it, `path` given by any of its spellings.
  pub fn holds(&self, spellings: &[&Path]) -> bool {
    let paths = lock(&self.paths);
    paths
      .iter()
      .any(|written| spellings.iter().any(|path| inside(written, path)))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_file_counts_as_written_while_its_guard_lives() {
    let writing = Writing::default();
    let guard = writing
      .hold(vec![PathBuf::from("/in/album/01.flac")])
      .unwrap();
    assert!(writing.holds(&[Path::new("/in/album/01.flac")]));
    assert!(
      writing.holds(&[Path::new("/in/album")]),
      "its directory too"
    );
    assert!(!writing.holds(&[Path::new("/in/album/02.flac")]));
    assert!(!writing.holds(&[Path::new("/in/alb")]), "by whole segments");
    drop(guard);
    assert!(!writing.holds(&[Path::new("/in/album/01.flac")]));
  }

  #[test]
  fn a_second_writer_of_one_file_is_refused_until_the_first_lets_go() {
    let writing = Writing::default();
    let file = PathBuf::from("/in/a_b/Album/01.flac");
    let first = writing.hold(vec![file.clone()]).unwrap();
    let refused = writing
      .hold(vec![PathBuf::from("/elsewhere/01.flac"), file.clone()])
      .err()
      .expect("held by one of its spellings");
    assert_eq!(refused.kind(), io::ErrorKind::ResourceBusy);
    assert_eq!(
      refused.to_string(),
      format!("another download is writing {}", file.display())
    );
    assert!(
      !writing.holds(&[Path::new("/elsewhere/01.flac")]),
      "a refused hold marks nothing"
    );
    let other_case = writing.hold(vec![PathBuf::from("/in/A_B/album/01.FLAC")]);
    assert_eq!(
      other_case.is_err(),
      cfg!(any(windows, target_os = "macos")),
      "names differing in case are one file where filesystems ignore case"
    );
    drop(other_case);
    drop(first);
    drop(writing.hold(vec![file]).unwrap());
  }
}
