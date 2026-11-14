//! Rules for paths that come from a request, a peer or the configuration, applied before such a
//! path is joined to a local directory. They read both `/` and `\` as separators whatever the
//! system, since a path from elsewhere may use either.
//!
//! There are two sets of rules for whether a path is absolute: Windows, where `X:\`, `X:/` and
//! `\\` start an absolute path, and everything else, where `/` does. Each rule takes the system
//! whose rules it applies, so both can be checked on any host.

mod local;
mod names;

use reseek_shares::traverses;

pub use local::{SEGMENT_BYTES, creatable, creatable_path, too_long, with_suffix};
pub use names::{
  directory_name_safely, file_name_safely, sanitize_filename, sanitize_path_segment,
  soulseek_qt_root_name,
};

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

  /// The separator the system writes between path segments.
  pub fn separator(self) -> char {
    match self {
      Os::Windows => '\\',
      Os::Unix => '/',
    }
  }

  /// The characters no file name may hold. `/` and `\` are replaced by the sanitizers on every
  /// system besides these.
  pub fn invalid_filename_chars(self) -> Vec<char> {
    match self {
      Os::Unix => vec!['\0', '/'],
      Os::Windows => {
        let mut chars = vec!['"', '<', '>', '|', '\0'];
        chars.extend((1u8..=31).map(char::from));
        chars.extend([':', '*', '?', '\\', '/']);
        chars
      }
    }
  }
}

/// Whether joining `segment` to a path would leave it under `os`'s rules: on Windows a segment
/// with a drive's `:` or starting with a separator joins relative to a drive, not to the path.
pub fn drive_relative(segment: &str, os: Os) -> bool {
  os == Os::Windows && (segment.contains(':') || segment.starts_with(['/', '\\']))
}

/// Whether `path` is absolute under `os`'s rules. `C:music` is relative to the drive's current
/// directory, so it is not, and `//server` is a UNC path only when written `\\server`.
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

/// `path` with every `/` and `\` replaced by `os`'s separator.
pub fn localize_path(path: &str, os: Os) -> String {
  let separator = os.separator();
  path
    .chars()
    .map(|c| if c == '/' || c == '\\' { separator } else { c })
    .collect()
}

/// `path` without the separators at its end.
fn trim_end_separators(path: &str) -> &str {
  path.trim_end_matches(['/', '\\'])
}

/// `segments` joined below `root` with `os`'s separator, refused when a segment is absolute,
/// holds a `.` or `..` segment, or on Windows names a drive, so the result is `root` or a path
/// under it.
pub fn combine_safely(root: &str, segments: &[&str], os: Os) -> Result<String, String> {
  if root.trim().is_empty() {
    return Err("The root must not be empty".to_string());
  }
  if traverses(root) {
    return Err("Path traversal is not permitted in the root".to_string());
  }
  let root = trim_end_separators(root);
  let mut combined = root.to_string();
  for segment in segments {
    if is_absolute(segment, os) || drive_relative(segment, os) {
      return Err("Absolute paths are not permitted in segments".to_string());
    }
    if traverses(segment) {
      return Err("Path traversal is not permitted in segments".to_string());
    }
    if segment.is_empty() {
      continue;
    }
    combined.push(os.separator());
    combined.push_str(segment);
  }
  let inside = combined == root || combined.starts_with(&format!("{root}{}", os.separator()));
  if !inside {
    return Err(format!(
      "Path traversal detected in combined path {combined}"
    ));
  }
  Ok(combined)
}

/// Refuses a path that is absolute under any system's rules, naming the field `name`.
pub fn relative_everywhere(name: &str, path: &str) -> Result<(), String> {
  if is_absolute(path, Os::Unix) || is_absolute(path, Os::Windows) {
    return Err(format!(
      "The field {name} must be a relative path on all operating systems."
    ));
  }
  Ok(())
}

/// Refuses a path with a `.` or `..` segment, naming the field `name`.
pub fn non_traversing(name: &str, path: &str) -> Result<(), String> {
  if traverses(path) {
    return Err(format!(
      "The field {name} contains one or more unsafe path traversal segments ('.' or '..')."
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
      !is_absolute("//server/share", Os::Windows),
      "UNC is \\\\ only"
    );
    assert!(
      !is_absolute("C:music", Os::Windows),
      "a drive without a separator"
    );
    assert!(!is_absolute("/music", Os::Windows));
    assert!(!is_absolute("", Os::Unix));
    assert!(relative_everywhere("x", "${SOURCE_DIRECTORY}").is_ok());
    assert!(relative_everywhere("x", "/x").is_err());
    assert!(relative_everywhere("x", "C:\\x").is_err());
  }

  #[test]
  fn combining_stays_below_the_root() {
    assert_eq!(
      combine_safely("/srv/", &["a", "", "b.flac"], Os::Unix).unwrap(),
      "/srv/a/b.flac"
    );
    assert_eq!(combine_safely("/srv", &[], Os::Unix).unwrap(), "/srv");
    assert_eq!(
      combine_safely("C:\\dl", &["x"], Os::Windows).unwrap(),
      "C:\\dl\\x"
    );
    let refused = |root, segment, os| combine_safely(root, &[segment], os).unwrap_err();
    assert!(refused("/srv", "/etc", Os::Unix).contains("Absolute"));
    assert!(refused("/srv", "../etc", Os::Unix).contains("traversal"));
    assert!(refused("/srv", "a/./b", Os::Unix).contains("traversal"));
    assert!(refused("C:\\dl", "D:x", Os::Windows).contains("Absolute"));
    assert!(refused("C:\\dl", "\\x", Os::Windows).contains("Absolute"));
    assert!(combine_safely("", &["x"], Os::Unix).is_err());
    assert!(combine_safely("/srv/..", &["x"], Os::Unix).is_err());
  }
}
