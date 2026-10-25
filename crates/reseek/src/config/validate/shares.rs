//! The shared directories: each entry's grammar, unique aliases and paths, and the share
//! filters.
//!
//! An entry is an optional `!` or `-` that excludes it, an optional `[alias]`, and an absolute
//! path. Without an alias the alias is the path's last segment. The first `]` ends the alias,
//! so a path may itself hold `]`.

use std::sync::LazyLock;

use regex::Regex;

use super::Report;
use super::fields::{duration_at_least, is_regex, range};
use crate::config::{MAX_SCAN_WORKERS, ShareCache, Shares};

const KEY: &str = "shares.directories";

/// An alias and nothing after it.
static NO_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(!|-)?(\[.*\])$").unwrap());
/// Nothing but separators after the optional alias: the root of a filesystem.
static ROOT: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(!|-)?(\[[^\]]*\])?[/\\]+$").unwrap());
/// An alias in brackets, then the path.
static ALIASED: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(!|-)?\[([^\]]*)\](.*)$").unwrap());
/// A segment of only dots.
static TRAVERSAL: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^\.+[/\\]?|[/\\]\.+[/\\]|[/\\]\.+$").unwrap());

/// One entry of `shares.directories`, taken apart.
#[derive(Debug, PartialEq, Eq)]
struct Entry<'a> {
  written: &'a str,
  alias: String,
  path: &'a str,
}

pub(super) fn check(shares: &Shares, report: &mut Report) {
  let mut entries = Vec::new();
  for written in &shares.directories {
    match entry(written) {
      Ok(entry) => entries.push(entry),
      Err(message) => report.add(KEY, message),
    }
  }
  for (i, a) in entries.iter().enumerate() {
    for b in &entries[i + 1..] {
      if a.alias == b.alias {
        report.add(
          KEY,
          format!(
            "Shares '{}', '{}' collide. Specify an alias for one or both to disambiguate them.",
            a.written, b.written
          ),
        );
      } else if a.path == b.path {
        report.add(
          KEY,
          format!(
            "Shares '{}', '{}' alias the same path",
            a.written, b.written
          ),
        );
      }
    }
  }
  for filter in &shares.filters {
    if !is_regex(filter) {
      report.add(
        "shares.filters",
        format!("Share filter '{filter}' is not a valid regular expression"),
      );
    }
  }
  check_cache(&shares.cache, report);
}

fn check_cache(cache: &ShareCache, report: &mut Report) {
  report.check(
    "shares.cache.workers",
    range("shares.cache.workers", cache.workers, 1, MAX_SCAN_WORKERS),
  );
  if let Some(retention) = cache.retention {
    report.check(
      "shares.cache.retention",
      duration_at_least(
        "shares.cache.retention",
        retention,
        std::time::Duration::from_secs(3600),
      ),
    );
  }
}

/// Takes an entry apart, or says what is wrong with it.
fn entry(written: &str) -> Result<Entry<'_>, String> {
  if ROOT.is_match(written) {
    return Err(format!(
      "Share {written} specifies a root mount, which is not supported."
    ));
  }
  let trimmed = written.trim_end_matches(['/', '\\']);
  if NO_PATH.is_match(trimmed) {
    return Err(format!("Share {written} does not specify a path"));
  }
  let (alias, path) = match ALIASED.captures(trimmed) {
    Some(captures) => {
      let alias = captures.get(2).map_or("", |m| m.as_str()).to_string();
      let path = captures.get(3).map_or("", |m| m.as_str());
      (alias, path)
    }
    None => {
      let path = trimmed.strip_prefix(['!', '-']).unwrap_or(trimmed);
      let alias = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
      (alias, path)
    }
  };
  if !(path.starts_with('/') || path.starts_with("\\\\") || has_drive(path)) {
    return Err(format!(
      "Share {written} contains a relative path; only absolute paths are supported."
    ));
  }
  if TRAVERSAL.is_match(path) {
    return Err(format!(
      "Share {written} contains an unsafe path traversal segment."
    ));
  }
  if alias.trim().is_empty() {
    return Err(format!(
      "Share '{written}' is invalid; alias may not be null, empty or consist of only whitespace"
    ));
  }
  if alias.contains(['/', '\\']) {
    return Err(format!(
      "Share '{written}' is invalid; aliases may not contain path separators '/' or '\\'"
    ));
  }
  Ok(Entry {
    written,
    alias,
    path,
  })
}

/// A drive letter and a colon, with or without a separator after it.
fn has_drive(path: &str) -> bool {
  let bytes = path.as_bytes();
  bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
  use super::*;

  fn problems(directories: &[&str]) -> Vec<String> {
    let shares = Shares {
      directories: directories.iter().map(|d| d.to_string()).collect(),
      ..Shares::default()
    };
    let mut report = Report::default();
    check(&shares, &mut report);
    report.messages()
  }

  #[test]
  fn entries_take_their_alias_from_brackets_or_the_last_segment() {
    assert_eq!(
      entry("/music/flac/").unwrap(),
      Entry {
        written: "/music/flac/",
        alias: "flac".into(),
        path: "/music/flac"
      }
    );
    assert_eq!(entry("![lossy]/music/mp3").unwrap().alias, "lossy");
    let bracket = entry("[a]/music/odd]name").unwrap();
    assert_eq!(
      (bracket.alias.as_str(), bracket.path),
      ("a", "/music/odd]name"),
      "the first ] ends the alias"
    );
    assert_eq!(entry("-C:\\Music").unwrap().alias, "Music");
    assert_eq!(entry("\\\\nas\\music").unwrap().alias, "music");
  }

  #[test]
  fn bad_entries_say_what_is_wrong() {
    assert_eq!(problems(&["[x]"]), ["Share [x] does not specify a path"]);
    assert_eq!(
      problems(&["[x]/"]),
      ["Share [x]/ specifies a root mount, which is not supported."]
    );
    assert_eq!(
      problems(&["/"]),
      ["Share / specifies a root mount, which is not supported."]
    );
    assert_eq!(
      problems(&["music"]),
      ["Share music contains a relative path; only absolute paths are supported."]
    );
    assert_eq!(
      problems(&["/x/.."]),
      ["Share /x/.. contains an unsafe path traversal segment."]
    );
    assert_eq!(
      problems(&["[ ]/music"]),
      ["Share '[ ]/music' is invalid; alias may not be null, empty or consist of only whitespace"]
    );
    assert_eq!(
      problems(&["[a\\b]/music"]),
      ["Share '[a\\b]/music' is invalid; aliases may not contain path separators '/' or '\\'"]
    );
  }

  #[test]
  fn aliases_and_paths_are_unique() {
    assert_eq!(
      problems(&["/a/music", "/b/music"]),
      [
        "Shares '/a/music', '/b/music' collide. Specify an alias for one or both to disambiguate \
         them."
      ]
    );
    assert_eq!(
      problems(&["[one]/music", "[two]/music/"]),
      ["Shares '[one]/music', '[two]/music/' alias the same path"]
    );
    assert!(problems(&["[one]/a/music", "/b/music"]).is_empty());
  }
}
