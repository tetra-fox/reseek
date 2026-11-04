//! The daemon's log files: the `.log` files directly in the log directory, and one of them as
//! text with ranges honored.

use std::path::Path;

use api_types::files::FilesystemFile;

use super::listing::file;
use crate::api::error::ApiError;
use crate::entries;

/// The `.log` files in `dir`, by name; 404 when the directory does not exist.
pub(super) fn files(dir: &Path) -> Result<Vec<FilesystemFile>, ApiError> {
  let entries = std::fs::read_dir(dir).map_err(|e| match e.kind() {
    std::io::ErrorKind::NotFound => ApiError::not_found("The log directory does not exist"),
    _ => ApiError::internal(format!("{} not read: {e}", dir.display())),
  })?;
  let mut found = Vec::new();
  for entry in entries.filter_map(|e| super::roots::entry_of(e, dir, "log files")) {
    let name = entry.file_name().to_string_lossy().into_owned();
    if !name.to_ascii_lowercase().ends_with(".log") {
      continue;
    }
    let Some(metadata) = super::roots::metadata_of(&entry.path(), "log files") else {
      continue;
    };
    if !metadata.is_file() || entries::left_out(&entry.file_name(), Some(&metadata)) {
      continue;
    }
    found.push(file(&name, name.clone(), false, &metadata));
  }
  found.sort_by(|a, b| a.name.cmp(&b.name));
  Ok(found)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn only_visible_log_files_are_listed() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["reseek-2.log", "reseek-1.LOG", ".old.log", "notes.txt"] {
      std::fs::write(dir.path().join(name), b"line\n").unwrap();
    }
    std::fs::create_dir(dir.path().join("dir.log")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("reseek-2.log"), dir.path().join("link.log"))
      .unwrap();
    let names: Vec<_> = files(dir.path())
      .unwrap()
      .into_iter()
      .map(|f| f.name)
      .collect();
    assert_eq!(names, ["reseek-1.LOG", "reseek-2.log"]);
    assert_eq!(files(&dir.path().join("none")).unwrap_err().status(), 404);
  }
}
