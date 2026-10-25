//! Path options: whether the file or directory a path names is there and usable. Whether a path
//! is absolute or steps outside where it starts is [`crate::safety`]'s.

use std::fs;
use std::path::{Path, PathBuf};

use crate::safety::{Os, is_absolute, non_traversing};

/// A path that is set is not the empty string, which would name the working directory.
pub(crate) fn not_empty(name: &str, path: &Path) -> Result<(), String> {
  if path.as_os_str().is_empty() {
    return Err(format!("The field {name} must be absent or a path."));
  }
  Ok(())
}

/// An absolute path for the running system that does not step outside itself.
pub(crate) fn absolute(name: &str, path: &Path) -> Result<(), String> {
  not_empty(name, path)?;
  let text = path.to_string_lossy();
  if !is_absolute(&text, Os::CURRENT) {
    return Err(format!(
      "The {name} field must be an absolute path on the current operating system."
    ));
  }
  non_traversing(name, &text)
}

/// A file that exists and can be opened for reading.
pub(crate) fn readable_file(name: &str, path: &Path) -> Result<(), String> {
  not_empty(name, path)?;
  let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
  if !full.is_file() {
    return Err(format!(
      "The {name} field specifies a non-existent file '{}'.",
      full.display()
    ));
  }
  fs::File::open(&full).map(drop).map_err(|_| {
    format!(
      "The {name} field specifies a file '{}' that cannot be opened for required access 'Read'",
      full.display()
    )
  })
}

/// A download directory: absolute, not stepping outside itself, and an existing writable
/// directory, unless it is `default`, which is made at startup. A path is compared to the
/// default after resolving links, so another spelling of the default is the default too.
pub(crate) fn download_directory(name: &str, path: &Path, default: &Path) -> Result<(), String> {
  absolute(name, path)?;
  if resolved(path) == resolved(default) {
    return Ok(());
  }
  existing_directory(name, path)?;
  writable(name, path)
}

/// The directory `path` names, under the program's directory, exists.
pub(crate) fn directory_under(name: &str, path: &Path, base: &Path) -> Result<(), String> {
  not_empty(name, path)?;
  let text = path.to_string_lossy();
  if path.has_root() || is_absolute(&text, Os::CURRENT) {
    return Err(format!(
      "The {name} field specifies a non-relative directory path: '{text}'."
    ));
  }
  existing_directory(name, &base.join(path))
}

fn existing_directory(name: &str, path: &Path) -> Result<(), String> {
  let full = resolved(path);
  if !full.is_dir() {
    return Err(format!(
      "The {name} field specifies a non-existent directory '{}'.",
      full.display()
    ));
  }
  Ok(())
}

/// [`crate::paths::probe`] with the message of a configured directory.
fn writable(name: &str, dir: &Path) -> Result<(), String> {
  crate::paths::probe(dir).map_err(|_| {
    format!(
      "The {name} field specifies a directory '{}' that exists, but is not writeable.",
      resolved(dir).display()
    )
  })
}

/// The path with its links resolved, or made absolute when it does not exist.
fn resolved(path: &Path) -> PathBuf {
  fs::canonicalize(path)
    .or_else(|_| std::path::absolute(path))
    .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn download_directories_must_exist_unless_default() {
    let dir = tempfile::tempdir().unwrap();
    let default = dir.path().join("downloads");
    assert!(
      download_directory("d", &default, &default).is_ok(),
      "the default is made at startup"
    );
    let missing = dir.path().join("elsewhere");
    assert_eq!(
      download_directory("d", &missing, &default).unwrap_err(),
      format!(
        "The d field specifies a non-existent directory '{}'.",
        missing.display()
      )
    );
    fs::create_dir(&missing).unwrap();
    assert!(download_directory("d", &missing, &default).is_ok());
    assert_eq!(
      download_directory("d", Path::new(""), &default).unwrap_err(),
      "The field d must be absent or a path."
    );
    assert!(download_directory("d", Path::new("relative"), &default).is_err());
  }

  #[cfg(unix)]
  #[test]
  fn a_link_to_the_default_is_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let default = dir.path().join("data/downloads");
    fs::create_dir_all(&default).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&default, &link).unwrap();
    // read-only, so a path that is not taken for the default fails the writable probe
    let mut perms = fs::metadata(&default).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o555);
    fs::set_permissions(&default, perms).unwrap();
    let result = download_directory("d", &link, &default);
    let mut perms = fs::metadata(&default).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    fs::set_permissions(&default, perms).unwrap();
    assert!(result.is_ok(), "{result:?}");
  }

  #[test]
  fn files_must_exist() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("blocklist.txt");
    assert!(
      readable_file("f", &file)
        .unwrap_err()
        .contains("non-existent file")
    );
    fs::write(&file, "1.2.3.4\n").unwrap();
    assert!(readable_file("f", &file).is_ok());
  }
}
