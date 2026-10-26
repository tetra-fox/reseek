//! The shared directories as configured: each entry taken apart into its alias, its path and
//! whether it excludes, the ordered list the scanner and the queries work from, and how a local
//! path is named to the network.
//!
//! An entry is an optional `!` or `-` that excludes it, an optional `[alias]` closed by the
//! first `]`, and an absolute path. Without an alias the alias is the path's last segment.
//! Every remote name starts with its share's alias and uses `\` between its parts.

use std::path::{Component, Path, PathBuf};

use sha1::{Digest, Sha1};

use crate::config::Options;

/// The option that adds the downloads directory as a share of its own.
pub const DERIVED_FROM_DOWNLOADS: &str = "shares.share_downloads";

/// One entry of the shared directories, taken apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
  /// the entry excludes its directory instead of sharing it
  pub excluded: bool,
  /// the name the directory is shared under
  pub alias: &'a str,
  /// the local path as written
  pub path: &'a str,
}

/// Takes apart an entry whose trailing separators are already removed.
pub fn entry(trimmed: &str) -> Entry<'_> {
  let (excluded, rest) = match trimmed.strip_prefix(['!', '-']) {
    Some(rest) => (true, rest),
    None => (false, trimmed),
  };
  if let Some(bracketed) = rest.strip_prefix('[')
    && let Some((alias, path)) = bracketed.split_once(']')
  {
    return Entry {
      excluded,
      alias,
      path,
    };
  }
  Entry {
    excluded,
    alias: rest.rsplit(['/', '\\']).next().unwrap_or(rest),
    path: rest,
  }
}

/// An entry without its trailing separators.
pub fn trimmed(raw: &str) -> &str {
  raw.trim_end_matches(['/', '\\'])
}

/// A shared or excluded directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Share {
  /// the upper-case hex SHA-1 of the alias
  pub id: String,
  /// the name the directory is shared under, the first part of every remote name in it
  pub alias: String,
  /// the entry excludes its directory instead of sharing it
  pub excluded: bool,
  /// the directory
  pub local_path: PathBuf,
  /// the entry as configured without trailing separators; the downloads path for the share
  /// the downloads option adds
  pub raw: String,
  /// the option the share comes from when it is not an entry of the shared directories
  pub derived_from: Option<&'static str>,
  /// directories in the share, its root included; `None` until counted
  pub directories: Option<u32>,
  /// files in the share; `None` until counted
  pub files: Option<u32>,
}

impl Share {
  /// The share an entry without trailing separators describes.
  pub fn parse(raw: &str, derived_from: Option<&'static str>) -> Self {
    let parts = entry(raw);
    Self {
      id: id(parts.alias),
      alias: parts.alias.to_string(),
      excluded: parts.excluded,
      local_path: PathBuf::from(parts.path),
      raw: raw.to_string(),
      derived_from,
      directories: None,
      files: None,
    }
  }

  /// The share as the API shows it.
  pub fn to_api(&self) -> api_types::shares::Share {
    api_types::shares::Share {
      id: self.id.clone(),
      alias: self.alias.clone(),
      is_excluded: self.excluded,
      local_path: self.local_path.to_string_lossy().into_owned(),
      raw: self.raw.clone(),
      remote_path: self.alias.clone(),
      directories: self.directories,
      files: self.files,
      derived_from: self.derived_from.map(str::to_string),
    }
  }
}

/// A share's id: the upper-case hex SHA-1 of its remote path, which is its alias.
pub fn id(alias: &str) -> String {
  Sha1::digest(alias.as_bytes())
    .iter()
    .map(|b| format!("{b:02X}"))
    .collect()
}

/// The shares of `options`, with the downloads directory `downloads` added when the options
/// say so and no configured share has its path: repeats dropped, deepest path first so that
/// the first share holding a directory is the one that owns it.
pub fn configure(options: &Options, downloads: &Path) -> Vec<Share> {
  let mut entries: Vec<(String, Option<&'static str>)> = Vec::new();
  for raw in &options.shares.directories {
    let raw = trimmed(raw).to_string();
    if !entries.iter().any(|(seen, _)| *seen == raw) {
      entries.push((raw, None));
    }
  }
  if options.shares.share_downloads {
    let path = downloads.to_string_lossy();
    let path = trimmed(&path).to_string();
    let shared = entries.iter().any(|(raw, _)| {
      let parts = entry(raw);
      !parts.excluded && same_path(Path::new(parts.path), Path::new(&path))
    });
    if !shared {
      entries.push((path, Some(DERIVED_FROM_DOWNLOADS)));
    }
  }
  let mut shares: Vec<Share> = entries
    .iter()
    .map(|(raw, derived)| Share::parse(raw, *derived))
    .collect();
  shares.sort_by_key(|share| std::cmp::Reverse(share.local_path.as_os_str().len()));
  shares
}

/// What identifies a share list, so applying the same list twice changes nothing: the SHA-1
/// of its entries in order.
pub fn fingerprint(shares: &[Share]) -> [u8; 20] {
  let joined = shares
    .iter()
    .map(|share| share.raw.as_str())
    .collect::<Vec<_>>()
    .join(";");
  Sha1::digest(joined.as_bytes()).into()
}

/// Whether paths on this system's usual filesystems differ by case alone. They do not on
/// Windows and macOS, so there two spellings of one path are the same path.
pub const CASE_SENSITIVE: bool = !cfg!(any(windows, target_os = "macos"));

fn same_name(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
  if CASE_SENSITIVE {
    a == b
  } else {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
  }
}

/// `path` relative to `root`, taken apart component by component, or `None` when `path` is
/// not `root` or under it.
pub fn relative<'a>(root: &Path, path: &'a Path) -> Option<Vec<Component<'a>>> {
  let mut rest = path.components();
  for part in root.components() {
    match rest.next() {
      Some(next) if same_name(next.as_os_str(), part.as_os_str()) => {}
      _ => return None,
    }
  }
  Some(rest.collect())
}

/// Whether `path` is `root` or under it.
pub fn is_within(path: &Path, root: &Path) -> bool {
  relative(root, path).is_some()
}

/// Whether two paths name the same directory, by this system's case rule.
pub fn same_path(a: &Path, b: &Path) -> bool {
  relative(a, b).is_some_and(|rest| rest.is_empty())
}

/// A local path's remote name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Masked {
  /// `alias\dir\file`, with a `\` inside a local name written as `_`
  pub name: String,
  /// a part of the local path was not UTF-8 and was written with U+FFFD in its place
  pub lossy: bool,
}

/// The remote name of `path`, which is `share`'s directory or under it: the alias, then each
/// part of the path below the share's directory.
pub fn mask(share: &Share, path: &Path) -> Option<Masked> {
  let parts = relative(&share.local_path, path)?;
  let mut name = share.alias.clone();
  let mut lossy = false;
  for part in parts {
    let os = part.as_os_str();
    lossy |= os.to_str().is_none();
    name.push('\\');
    name.push_str(&os.to_string_lossy().replace('\\', "_"));
  }
  Some(Masked { name, lossy })
}

/// The share that owns `path`: the deepest shared directory that is `path` or holds it.
/// `shares` is ordered deepest first.
pub fn owner<'a>(shares: &'a [Share], path: &Path) -> Option<&'a Share> {
  shares
    .iter()
    .find(|share| !share.excluded && is_within(path, &share.local_path))
}

/// Whether `path` is an excluded directory or under one.
pub fn excluded(shares: &[Share], path: &Path) -> bool {
  shares
    .iter()
    .any(|share| share.excluded && is_within(path, &share.local_path))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn entries_take_the_alias_up_to_the_first_bracket() {
    assert_eq!(
      entry("[foo]/bar/[baz]"),
      Entry {
        excluded: false,
        alias: "foo",
        path: "/bar/[baz]"
      }
    );
    assert_eq!(
      entry("!-/x").path,
      "-/x",
      "only the first character excludes"
    );
    assert_eq!(entry("-/mnt/music").alias, "music");
    assert_eq!(entry("D:\\Music").alias, "Music");
    assert_eq!(
      entry("[open/x").alias,
      "x",
      "an unclosed bracket is no alias"
    );
  }

  #[test]
  fn ids_are_the_upper_case_sha1_of_the_alias() {
    assert_eq!(id("a"), "86F7E437FAA5A7FCE15D1DDCB9EAEAEA377667B8");
  }

  #[test]
  fn the_list_drops_repeats_puts_deepest_first_and_adds_the_downloads() {
    let mut options = Options::default();
    options.shares.directories = vec![
      "/mnt/music/".into(),
      "/mnt/music".into(),
      "[Deep]/mnt/music/flac/lossless".into(),
      "!/mnt/music/private".into(),
    ];
    options.shares.share_downloads = true;
    let shares = configure(&options, Path::new("/srv/downloads"));
    let raws: Vec<&str> = shares.iter().map(|s| s.raw.as_str()).collect();
    assert_eq!(
      raws,
      [
        "[Deep]/mnt/music/flac/lossless",
        "!/mnt/music/private",
        "/srv/downloads",
        "/mnt/music"
      ]
    );
    assert_eq!(shares[2].derived_from, Some(DERIVED_FROM_DOWNLOADS));
    assert_eq!(shares[2].alias, "downloads");
    options
      .shares
      .directories
      .push("[dl]/srv/downloads/".into());
    let shares = configure(&options, Path::new("/srv/downloads"));
    assert!(
      shares.iter().all(|s| s.derived_from.is_none()),
      "a configured share of the same path wins"
    );
  }

  #[test]
  fn masking_is_by_component_and_keeps_backslashes_out_of_names() {
    let share = Share::parse("[Music]/mnt/music", None);
    let masked = |path: &str| mask(&share, Path::new(path)).map(|m| m.name);
    assert_eq!(masked("/mnt/music").as_deref(), Some("Music"));
    assert_eq!(
      masked("/mnt/music/A/mnt/music/01.flac").as_deref(),
      Some("Music\\A\\mnt\\music\\01.flac"),
      "the root repeated deeper stays a plain name"
    );
    assert_eq!(masked("/mnt/musical/x"), None);
    #[cfg(unix)]
    assert_eq!(
      masked("/mnt/music/a\\b.mp3").as_deref(),
      Some("Music\\a_b.mp3")
    );
  }

  #[cfg(unix)]
  #[test]
  fn a_name_that_is_not_utf8_is_masked_lossily() {
    use std::os::unix::ffi::OsStrExt;
    let share = Share::parse("/m", None);
    let path = Path::new("/m").join(std::ffi::OsStr::from_bytes(b"caf\xe9.mp3"));
    let masked = mask(&share, &path).unwrap();
    assert!(masked.lossy);
    assert_eq!(masked.name, "m\\caf\u{fffd}.mp3");
  }

  #[test]
  fn the_deepest_share_owns_a_directory() {
    let mut options = Options::default();
    options.shares.directories = vec![
      "/mnt/music".into(),
      "[Flac]/mnt/music/flac".into(),
      "!/mnt/music/private".into(),
    ];
    let shares = configure(&options, Path::new("/d"));
    let owned = |p: &str| owner(&shares, Path::new(p)).map(|s| s.alias.as_str());
    assert_eq!(owned("/mnt/music/flac/x"), Some("Flac"));
    assert_eq!(owned("/mnt/music/mp3"), Some("music"));
    assert_eq!(owned("/elsewhere"), None);
    assert!(excluded(&shares, Path::new("/mnt/music/private/x")));
    assert!(!excluded(&shares, Path::new("/mnt/music/privateer")));
  }
}
