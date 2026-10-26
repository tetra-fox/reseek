//! Rules for paths that come from a request, a peer or the configuration, applied before such a
//! path is joined to a local directory. They read both `/` and `\` as separators whatever the
//! system, since a path from elsewhere may use either.
//!
//! There are two sets of rules for whether a path is absolute: Windows, where `X:\`, `X:/` and
//! `\\` start an absolute path, and everything else, where `/` does. Each rule takes the system
//! whose rules it applies, so both can be checked on any host.

mod local;
mod names;

pub use local::{SEGMENT_BYTES, creatable, creatable_path, path_limit, too_long};
pub use names::{
  directory_name_safely, file_name_safely, sanitize_filename, sanitize_path, sanitize_path_segment,
  strip_path_root,
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

  /// The characters no file name may hold. `/` and `\` are replaced by the sanitisers on every
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

/// Whether a segment of `path`, split on `/` and `\`, is `.` or `..`.
pub fn traverses(path: &str) -> bool {
  path.split(['/', '\\']).any(|s| s == "." || s == "..")
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

/// Whether `sub` is `root` or a directory under it. Both must be absolute without `.` or `..`
/// segments; names compare exactly, ignoring case on Windows.
pub fn is_sub_directory_of(sub: &str, root: &str, os: Os) -> Result<bool, String> {
  for path in [sub, root] {
    if !is_absolute(path, os) {
      return Err(format!("{path} is not an absolute path"));
    }
    if traverses(path) {
      return Err(format!("{path} contains a traversal segment"));
    }
  }
  let fold = |text: &str| match os {
    Os::Windows => text.to_lowercase(),
    Os::Unix => text.to_string(),
  };
  let sub = fold(&localize_path(trim_end_separators(sub), os));
  let root = fold(&localize_path(trim_end_separators(root), os));
  Ok(sub == root || sub.starts_with(&format!("{root}{}", os.separator())))
}

/// `segments` joined below `root` with `os`'s separator, refused when a segment is absolute,
/// holds a `.` or `..` segment, or on Windows names a drive, so the result is `root` or a path
/// under it.
pub fn combine_safely(root: &str, segments: &[&str], os: Os) -> Result<String, String> {
  if root.trim().is_empty() {
    return Err("the root may not be empty".to_string());
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
  fn traversal_is_a_whole_dot_segment() {
    for path in ["foo/../bar", "..", "./x", "/..", ".\\b", "a/."] {
      assert!(traverses(path), "{path}");
    }
    for path in ["foo..bar", ".hidden", "a/..b/c.", "/a/b"] {
      assert!(!traverses(path), "{path}");
    }
  }

  #[test]
  fn invalid_characters_are_the_systems() {
    assert_eq!(Os::Unix.invalid_filename_chars(), ['\0', '/']);
    let windows = Os::Windows.invalid_filename_chars();
    for c in [
      '"', '<', '>', '|', '\0', '\u{1}', '\u{1f}', ':', '*', '?', '\\', '/',
    ] {
      assert!(windows.contains(&c), "{c:?}");
    }
    assert!(!windows.contains(&' '));
  }

  #[test]
  fn paths_are_localised_to_the_systems_separator() {
    assert_eq!(localize_path("a/b\\c", Os::Unix), "a/b/c");
    assert_eq!(localize_path("a/b\\c", Os::Windows), "a\\b\\c");
  }

  #[test]
  fn a_sub_directory_is_the_root_or_below_it() {
    let unix = |sub, root| is_sub_directory_of(sub, root, Os::Unix).unwrap();
    assert!(unix("/srv/downloads", "/srv/downloads/"));
    assert!(unix("/srv/downloads/a/b", "/srv/downloads"));
    assert!(!unix("/srv/downloadsx", "/srv/downloads"));
    assert!(!unix("/srv/Downloads/a", "/srv/downloads"), "exact on Unix");
    let windows = |sub, root| is_sub_directory_of(sub, root, Os::Windows).unwrap();
    assert!(windows("C:\\Downloads\\x", "c:\\downloads"));
    assert!(windows("C:/Downloads/x", "C:\\Downloads"));
    assert!(is_sub_directory_of("relative", "/srv", Os::Unix).is_err());
    assert!(is_sub_directory_of("/srv/../etc", "/srv", Os::Unix).is_err());
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
