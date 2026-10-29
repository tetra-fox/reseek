//! Names from elsewhere made safe to use as local names: the roots other systems put at the
//! front of a path stripped, characters no file name may hold replaced, and the file and
//! directory parts of a remote path taken out.

use std::sync::LazyLock;

use regex::Regex;

use super::{Os, localize_path};

/// A drive root: `C:` with or without a separator.
static DRIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z]:[/\\]?").unwrap());
/// A UNC root: two separators, the server's name and an optional separator.
static UNC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[/\\]{2}[^/\\]+[/\\]?").unwrap());
/// The root SoulseekQt puts before its shares: `@@` and at least five letters or digits.
static SOULSEEK_QT: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^@@[A-Za-z0-9]{5,}[/\\]?").unwrap());

/// `path` localized to `os` without a leading drive root, UNC root (with its server) or
/// SoulseekQt root. A single leading `/` is not a root and stays.
pub fn strip_path_root(path: &str, os: Os) -> String {
  let mut path = localize_path(path, os);
  for root in [&*DRIVE, &*UNC, &*SOULSEEK_QT] {
    path = root.replace(&path, "").into_owned();
  }
  path
}

/// Refuses a replacement that is a separator or itself a character `os` refuses in names.
fn usable_replacement(replacement: char, os: Os) -> Result<(), String> {
  if replacement == '/' || replacement == '\\' || os.invalid_filename_chars().contains(&replacement)
  {
    return Err(format!(
      "{replacement:?} may not replace characters in a file name"
    ));
  }
  Ok(())
}

/// `name` with every character `os` refuses in a file name, and every `/` and `\` on every
/// system, replaced by `replacement`.
pub fn sanitize_filename(name: &str, replacement: char, os: Os) -> Result<String, String> {
  usable_replacement(replacement, os)?;
  let invalid = os.invalid_filename_chars();
  Ok(
    name
      .chars()
      .map(|c| {
        if invalid.contains(&c) || c == '/' || c == '\\' {
          replacement
        } else {
          c
        }
      })
      .collect(),
  )
}

/// One path segment made safe: [`sanitize_filename`], then a `.` or `..` that remains becomes
/// `replacement`, or nothing when the replacement is itself `.`.
pub fn sanitize_path_segment(segment: &str, replacement: char, os: Os) -> Result<String, String> {
  let sanitized = sanitize_filename(segment, replacement, os)?;
  if sanitized == "." || sanitized == ".." {
    return Ok(if replacement == '.' {
      String::new()
    } else {
      replacement.to_string()
    });
  }
  Ok(sanitized)
}

/// [`sanitize_path_segment`] with `_` as the replacement.
fn underscored(segment: &str, os: Os) -> String {
  // `_` is no separator and no character any system forbids, so it is never refused
  sanitize_path_segment(segment, '_', os).unwrap_or_default()
}

/// A relative path made of `path`'s segments, each made safe, joined with `os`'s separator:
/// empty segments and segments that end up empty are dropped, so a leading root goes too.
pub fn sanitize_path(path: &str, replacement: char, os: Os) -> Result<String, String> {
  let mut segments = Vec::new();
  for segment in path.split(['/', '\\']).filter(|s| !s.is_empty()) {
    let safe = sanitize_path_segment(segment, replacement, os)?;
    if !safe.is_empty() {
      segments.push(safe);
    }
  }
  Ok(segments.join(&os.separator().to_string()))
}

/// The file name a remote path ends in, made safe unless `sanitize` is false; `None` for an
/// empty path, one that ends in a separator, or one that is only a root.
pub fn file_name_safely(path: &str, sanitize: bool, os: Os) -> Option<String> {
  if path.is_empty() || path.ends_with(['/', '\\']) || strip_path_root(path, os).is_empty() {
    return None;
  }
  let localized = localize_path(path, os);
  let name = localized.rsplit(os.separator()).next().unwrap_or_default();
  if sanitize {
    Some(underscored(name, os))
  } else {
    Some(name.to_string())
  }
}

/// The directory part of a remote path: every segment but the last, made safe unless
/// `sanitize` is false, joined with `os`'s separator. The root is left off unless
/// `retain_root`, which keeps it with the leading separators the path had. `None` for an empty
/// path, a bare name, or a path with no directory under its root.
pub fn directory_name_safely(
  path: &str,
  retain_root: bool,
  sanitize: bool,
  os: Os,
) -> Option<String> {
  if path.is_empty() || !path.contains(['/', '\\']) {
    return None;
  }
  let leading = path.chars().take_while(|c| *c == '/' || *c == '\\').count();
  let separator = os.separator();
  let localized = localize_path(path, os);
  let localized = localized.trim_end_matches(separator);
  let stripped = strip_path_root(localized, os);
  if stripped.is_empty() {
    return None;
  }
  let base = if retain_root { localized } else { &stripped };
  let segments: Vec<&str> = base.split(separator).filter(|s| !s.is_empty()).collect();
  let (_, directories) = segments.split_last()?;
  if directories.is_empty() {
    return None;
  }
  let directories = directories
    .iter()
    .map(|segment| {
      if sanitize {
        underscored(segment, os)
      } else {
        segment.to_string()
      }
    })
    .collect::<Vec<_>>()
    .join(&separator.to_string());
  if retain_root {
    Some(format!(
      "{}{directories}",
      separator.to_string().repeat(leading)
    ))
  } else {
    Some(directories)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn roots_of_other_systems_are_stripped() {
    let unix = |p| strip_path_root(p, Os::Unix);
    assert_eq!(unix("C:\\Music\\Artist"), "Music/Artist");
    assert_eq!(unix("Z:"), "");
    assert_eq!(unix("//server/share/Music"), "share/Music");
    assert_eq!(unix("\\\\server"), "");
    assert_eq!(unix("@@abcde/Music"), "Music");
    assert_eq!(unix("@@abcd/Music"), "@@abcd/Music", "four is not a root");
    assert_eq!(unix("/home/user"), "/home/user");
    assert_eq!(strip_path_root("\\Music", Os::Windows), "\\Music");
  }

  #[test]
  fn file_names_lose_the_characters_their_system_refuses() {
    assert_eq!(
      sanitize_filename("has/slash", '_', Os::Unix).unwrap(),
      "has_slash"
    );
    assert_eq!(
      sanitize_filename("back\\slash", '_', Os::Unix).unwrap(),
      "back_slash"
    );
    assert_eq!(
      sanitize_filename("file:name", '_', Os::Windows).unwrap(),
      "file_name"
    );
    assert_eq!(
      sanitize_filename("file:name", '_', Os::Unix).unwrap(),
      "file:name"
    );
    assert!(sanitize_filename("x", '/', Os::Unix).is_err());
    assert!(sanitize_filename("x", ':', Os::Windows).is_err());
  }

  #[test]
  fn dot_segments_become_the_replacement() {
    assert_eq!(sanitize_path_segment("..", '_', Os::Unix).unwrap(), "_");
    assert_eq!(sanitize_path_segment(".", '_', Os::Unix).unwrap(), "_");
    assert_eq!(sanitize_path_segment("..", '.', Os::Unix).unwrap(), "");
    for name in [
      "\u{dc}n\u{ef}c\u{f6}d\u{e9}",
      "\u{43f}\u{43e}\u{43b}\u{44c}",
    ] {
      assert_eq!(sanitize_path_segment(name, '_', Os::Unix).unwrap(), name);
    }
  }

  #[test]
  fn paths_are_sanitized_segment_by_segment() {
    let unix = |p| sanitize_path(p, '_', Os::Unix).unwrap();
    assert_eq!(unix("foo/../bar"), "foo/_/bar");
    assert_eq!(unix("/foo/bar"), "foo/bar");
    assert_eq!(unix("foo//bar"), "foo/bar");
    assert_eq!(unix("../.."), "_/_");
    assert_eq!(unix("./."), "_/_");
    assert_eq!(
      sanitize_path("a/b", '_', Os::Windows).unwrap(),
      "a\\b",
      "joined with the system's separator"
    );
  }

  #[test]
  fn the_file_name_is_the_last_segment() {
    let unix = |p, sanitize| file_name_safely(p, sanitize, Os::Unix);
    assert_eq!(unix("foo//bar.flac", true).as_deref(), Some("bar.flac"));
    assert_eq!(unix("foo/.", true).as_deref(), Some("_"));
    assert_eq!(unix("foo/.", false).as_deref(), Some("."));
    assert_eq!(
      unix("@@abcde\\song.flac", true).as_deref(),
      Some("song.flac")
    );
    for none in ["", "foo/", "C:", "//server", "@@abcde"] {
      assert_eq!(unix(none, true), None, "{none:?}");
    }
  }

  #[test]
  fn the_directory_is_every_segment_but_the_last() {
    let unix = |p, retain, sanitize| directory_name_safely(p, retain, sanitize, Os::Unix);
    assert_eq!(
      unix("C:\\Music\\song.flac", false, true).as_deref(),
      Some("Music")
    );
    assert_eq!(
      unix("C:\\song.flac", true, true).as_deref(),
      Some("C:"),
      "a colon is a name character on Unix"
    );
    assert_eq!(
      directory_name_safely("C:\\song.flac", true, true, Os::Windows).as_deref(),
      Some("C_")
    );
    assert_eq!(
      unix("//server/file.txt", true, true).as_deref(),
      Some("//server")
    );
    assert_eq!(
      directory_name_safely("//server/file.txt", true, true, Os::Windows).as_deref(),
      Some("\\\\server")
    );
    assert_eq!(unix("@@abcde/file.txt", false, true), None);
    assert_eq!(
      unix("@@abcde/file.txt", true, true).as_deref(),
      Some("@@abcde")
    );
    assert_eq!(
      unix("foo/../bar/file.txt", false, true).as_deref(),
      Some("foo/_/bar")
    );
    assert_eq!(
      unix("foo/../bar/file.txt", false, false).as_deref(),
      Some("foo/../bar")
    );
    assert_eq!(
      unix("Artist/Album/", false, true).as_deref(),
      Some("Artist")
    );
    assert_eq!(unix("fo\0o/bar", false, true).as_deref(), Some("fo_o"));
    assert_eq!(unix("file.txt", false, true), None, "a bare name is a file");
    assert_eq!(unix("", false, true), None);
  }
}
