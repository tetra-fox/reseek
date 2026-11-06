//! Names for the files and directories the daemon creates from names that came from elsewhere:
//! a name a system would change or refuse when it is created, or one too long for it, is
//! changed first, so the file is created under the name the daemon chose.

use super::Os;
use super::names::sanitize_path;

/// The most bytes of UTF-8 one path segment may take.
pub const SEGMENT_BYTES: usize = 255;

/// The device names Windows reserves, with or without an extension.
const RESERVED: [&str; 22] = [
  "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
  "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// The longest path `os` creates: 4096 bytes on Unix, 260 characters on Windows without long
/// path support.
fn path_limit(os: Os) -> usize {
  match os {
    Os::Unix => 4096,
    Os::Windows => 260,
  }
}

/// Whether `path` is longer than `os` creates.
pub fn too_long(path: &str, os: Os) -> bool {
  match os {
    Os::Unix => path.len() > path_limit(os),
    Os::Windows => path.chars().count() > path_limit(os),
  }
}

/// `segment`, a name already sanitized, as `os` creates it unchanged: on Windows, trailing
/// dots and spaces, which Windows drops, are dropped here, and a reserved device name gets `_`
/// after its stem; everywhere, a name over 255 bytes has its stem cut to fit, its extension
/// kept. A name that ends up empty is `_`.
pub fn creatable(segment: &str, os: Os) -> String {
  let mut name = segment.to_string();
  if os == Os::Windows {
    name.truncate(name.trim_end_matches(['.', ' ']).len());
    let stem_end = name.find('.').unwrap_or(name.len());
    if RESERVED
      .iter()
      .any(|reserved| name[..stem_end].eq_ignore_ascii_case(reserved))
    {
      name.insert(stem_end, '_');
    }
  }
  if name.is_empty() {
    return "_".to_string();
  }
  if name.len() <= SEGMENT_BYTES {
    return name;
  }
  with_suffix(&name, "")
}

/// `name` with `suffix` put before its extension, the stem cut at a character boundary so the
/// result takes at most [`SEGMENT_BYTES`]. `suffix` is shorter than half of that.
pub fn with_suffix(name: &str, suffix: &str) -> String {
  let (stem, extension) = match name.rfind('.') {
    // an extension too long to leave room for a stem is cut with the rest
    Some(dot) if dot > 0 && name.len() - dot < SEGMENT_BYTES / 2 => name.split_at(dot),
    _ => (name, ""),
  };
  let mut end = stem
    .len()
    .min(SEGMENT_BYTES - extension.len() - suffix.len());
  while !stem.is_char_boundary(end) {
    end -= 1;
  }
  format!("{}{suffix}{extension}", &stem[..end])
}

/// `path` as relative segments `os` creates unchanged: [`sanitize_path`], then each segment
/// [`creatable`].
pub fn creatable_path(path: &str, os: Os) -> String {
  // `_` is no separator and no character any system forbids, so it is never refused
  let sanitized = sanitize_path(path, '_', os).unwrap_or_default();
  sanitized
    .split(os.separator())
    .filter(|segment| !segment.is_empty())
    .map(|segment| creatable(segment, os))
    .collect::<Vec<_>>()
    .join(&os.separator().to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn windows_reserved_names_get_an_underscore_after_their_stem() {
    let windows = |name: &str| creatable(name, Os::Windows);
    assert_eq!(windows("CON"), "CON_");
    assert_eq!(windows("con.txt"), "con_.txt");
    assert_eq!(windows("LPT9.tar.gz"), "LPT9_.tar.gz");
    assert_eq!(windows("CONSOLE.txt"), "CONSOLE.txt");
    assert_eq!(windows("COM10"), "COM10");
    assert_eq!(
      creatable("CON", Os::Unix),
      "CON",
      "only Windows reserves them"
    );
  }

  #[test]
  fn windows_trailing_dots_and_spaces_are_dropped() {
    assert_eq!(creatable("Album. . ", Os::Windows), "Album");
    assert_eq!(creatable("...", Os::Windows), "_");
    assert_eq!(creatable("NUL. ", Os::Windows), "NUL_");
    assert_eq!(creatable("Album. ", Os::Unix), "Album. ");
  }

  #[test]
  fn long_names_keep_their_extension_and_whole_characters() {
    let long = format!("{}.flac", "é".repeat(200));
    let cut = creatable(&long, Os::Unix);
    assert!(cut.len() <= SEGMENT_BYTES, "{}", cut.len());
    assert!(cut.ends_with("é.flac"));
    let no_extension = "x".repeat(300);
    assert_eq!(creatable(&no_extension, Os::Unix).len(), SEGMENT_BYTES);
    let huge_extension = format!("a.{}", "y".repeat(300));
    assert_eq!(creatable(&huge_extension, Os::Unix).len(), SEGMENT_BYTES);
  }

  #[test]
  fn a_path_is_sanitized_then_made_creatable_segment_by_segment() {
    assert_eq!(
      creatable_path("Artist\\..\\AUX\\track?.flac", Os::Windows),
      "Artist\\_\\AUX_\\track_.flac"
    );
    assert_eq!(creatable_path("/a//b/../c", Os::Unix), "a/b/_/c");
  }

  #[test]
  fn paths_are_measured_as_their_system_measures_them() {
    assert!(!too_long(&"é".repeat(2048), Os::Unix));
    assert!(too_long(&"é".repeat(2049), Os::Unix));
    assert!(!too_long(&"é".repeat(260), Os::Windows));
    assert!(too_long(&"é".repeat(261), Os::Windows));
  }
}
