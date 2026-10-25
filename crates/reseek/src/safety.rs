//! Rules for paths that come from a request, a peer or the configuration, applied before such a
//! path is joined to a local directory. They read both `/` and `\` as separators whatever the
//! system, since a path from elsewhere may use either.
//!
//! There are two sets of rules for whether a path is absolute: Windows, where `X:\`, `X:/` and
//! `\\` start an absolute path, and everything else, where `/` does.

/// Which operating system's path rules a check uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
  /// `X:\`, `X:/` and `\\` start an absolute path
  Windows,
  /// `/` starts an absolute path
  Unix,
}

impl Os {
  /// The rules of the system the program runs on.
  pub const CURRENT: Os = if cfg!(windows) { Os::Windows } else { Os::Unix };
}

/// Whether `path` is absolute under `os`'s rules. `C:music` is relative to the drive's current
/// directory, so it is not.
pub fn is_absolute(path: &str, os: Os) -> bool {
  match os {
    Os::Unix => path.starts_with('/'),
    Os::Windows => {
      let bytes = path.as_bytes();
      path.starts_with("\\\\")
        || (bytes.len() >= 3
          && bytes[0].is_ascii_alphabetic()
          && bytes[1] == b':'
          && matches!(bytes[2], b'\\' | b'/'))
    }
  }
}

/// Whether a segment of `path`, split on `/` and `\`, is `.` or `..`.
pub fn traverses(path: &str) -> bool {
  path.split(['/', '\\']).any(|s| s == "." || s == "..")
}

/// Refuses a path that is absolute under any system's rules, naming the field `name`.
pub fn relative_everywhere(name: &str, path: &str) -> Result<(), String> {
  if is_absolute(path, Os::Unix) || is_absolute(path, Os::Windows) {
    return Err(format!(
      "The {name} field must be a relative path on all operating systems."
    ));
  }
  Ok(())
}

/// Refuses a path with a `.` or `..` segment, naming the field `name`.
pub fn non_traversing(name: &str, path: &str) -> Result<(), String> {
  if traverses(path) {
    return Err(format!(
      "The {name} field contains one or more unsafe path traversal segments ('.' or '..')"
    ));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn absolute_paths_follow_each_systems_rules() {
    assert!(is_absolute("/music", Os::Unix));
    assert!(!is_absolute("music", Os::Unix));
    assert!(!is_absolute("C:\\music", Os::Unix));
    assert!(is_absolute("C:\\music", Os::Windows));
    assert!(is_absolute("c:/music", Os::Windows));
    assert!(is_absolute("\\\\server\\share", Os::Windows));
    assert!(
      !is_absolute("C:music", Os::Windows),
      "a drive without a separator"
    );
    assert!(!is_absolute("/music", Os::Windows));
    assert!(relative_everywhere("x", "${SOURCE_DIRECTORY}").is_ok());
    assert!(relative_everywhere("x", "/x").is_err());
    assert!(relative_everywhere("x", "C:\\x").is_err());
  }

  #[test]
  fn traversal_is_a_whole_dot_segment() {
    assert!(traverses("a/../b"));
    assert!(traverses(".\\b"));
    assert!(traverses("a/."));
    assert!(!traverses("a/..b/c."));
    assert!(!traverses("/a/b"));
  }
}
