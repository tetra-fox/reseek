//! The files downloads are writing now. Such a file is not served or deleted, and pruning
//! leaves it alone, until the download lets go of it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

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

fn lock(paths: &Mutex<Vec<PathBuf>>) -> MutexGuard<'_, Vec<PathBuf>> {
  // every holder of the lock changes the list whole
  paths.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Writing {
  /// Marks one file as being written, by each of `spellings`: the path as written under the
  /// configured directory and the same path resolved through links.
  pub fn hold(&self, spellings: Vec<PathBuf>) -> WriteGuard {
    lock(&self.paths).extend(spellings.iter().cloned());
    WriteGuard {
      paths: self.paths.clone(),
      held: spellings,
    }
  }

  /// Whether a file being written is `path` or under it, `path` given by any of its spellings.
  pub fn holds(&self, spellings: &[&Path]) -> bool {
    let paths = lock(&self.paths);
    paths
      .iter()
      .any(|written| spellings.iter().any(|path| written.starts_with(path)))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_file_counts_as_written_while_its_guard_lives() {
    let writing = Writing::default();
    let guard = writing.hold(vec![PathBuf::from("/in/album/01.flac")]);
    let second = writing.hold(vec![PathBuf::from("/in/album/01.flac")]);
    assert!(writing.holds(&[Path::new("/in/album/01.flac")]));
    assert!(
      writing.holds(&[Path::new("/in/album")]),
      "its directory too"
    );
    assert!(!writing.holds(&[Path::new("/in/album/02.flac")]));
    assert!(!writing.holds(&[Path::new("/in/alb")]), "by whole segments");
    drop(guard);
    assert!(
      writing.holds(&[Path::new("/in/album/01.flac")]),
      "held twice"
    );
    drop(second);
    assert!(!writing.holds(&[Path::new("/in/album/01.flac")]));
  }
}
