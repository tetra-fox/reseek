//! The shared directories: each entry's grammar, unique aliases and paths, and the share
//! filters.
//!
//! An entry is an optional `!` or `-` that excludes it, an optional `[alias]`, and an absolute
//! path. Without an alias the alias is the path's last segment. The first `]` ends the alias,
//! so a path may itself hold `]`.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::Report;
use super::fields::{duration_at_least, is_regex, range};
use crate::config::{MAX_SCAN_WORKERS, ShareCache, Shares};
use crate::shares::share::{self, DERIVED_FROM_DOWNLOADS, Share, trimmed};

const KEY: &str = "shares.directories";

/// An alias and nothing after it.
static NO_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(!|-)?(\[.*\])$").unwrap());
/// Nothing but separators after the optional alias: the root of a filesystem.
static ROOT: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(!|-)?(\[[^\]]*\])?[/\\]+$").unwrap());
/// A segment of only dots.
static TRAVERSAL: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^\.+[/\\]?|[/\\]\.+[/\\]|[/\\]\.+$").unwrap());

/// One share of `shares.directories`, or the share `shares.share_downloads` adds, with the
/// entry as written.
struct Entry<'a> {
  written: &'a str,
  share: &'a Share,
}

impl Entry<'_> {
  fn derived(&self) -> bool {
    self.share.derived_from.is_some()
  }

  /// How a message names the entry.
  fn named(&self) -> String {
    if self.derived() {
      format!("'{}' (from {DERIVED_FROM_DOWNLOADS})", self.written)
    } else {
      format!("'{}'", self.written)
    }
  }
}

/// Checks the shared directories, with the share of the downloads directory `downloads` when
/// `shares.share_downloads` adds it, and the filters and cache options.
pub(super) fn check(shares: &Shares, downloads: &Path, case_sensitive: bool, report: &mut Report) {
  let mut invalid: Vec<&str> = Vec::new();
  for written in &shares.directories {
    if let Err(message) = valid(written) {
      report.add(KEY, message);
      invalid.push(trimmed(written));
    }
  }
  let list = share::in_written_order(shares, downloads);
  let entries: Vec<Entry> = list
    .iter()
    .filter(|share| !invalid.contains(&share.raw.as_str()))
    .map(|share| Entry {
      // a share is named by its entry as first written, trailing separators and all; the
      // downloads share has no entry
      written: shares
        .directories
        .iter()
        .find(|written| share.derived_from.is_none() && trimmed(written) == share.raw)
        .map_or(share.raw.as_str(), String::as_str),
      share,
    })
    .collect();
  for (i, a) in entries.iter().enumerate() {
    for b in &entries[i + 1..] {
      let (first, second) = if b.derived() { (b, a) } else { (a, b) };
      if a.share.alias.to_lowercase() == b.share.alias.to_lowercase() {
        let message = if first.derived() {
          format!(
            "Share {} collides with {}. Specify an alias for {} to disambiguate them.",
            first.named(),
            second.named(),
            second.named()
          )
        } else {
          format!(
            "Shares {} and {} collide. Specify an alias for one or both to disambiguate them.",
            a.named(),
            b.named()
          )
        };
        report.add(KEY, message);
      } else if share::same_path(&a.share.local_path, &b.share.local_path) {
        let message = if first.derived() {
          format!(
            "Share {} aliases the same path as {}.",
            first.named(),
            second.named()
          )
        } else {
          format!(
            "Shares {} and {} alias the same path.",
            a.named(),
            b.named()
          )
        };
        report.add(KEY, message);
      }
    }
  }
  for filter in &shares.filters {
    if !is_regex(filter, case_sensitive) {
      report.add(
        "shares.filters",
        format!("Share filter '{filter}' is not a valid regular expression."),
      );
    }
  }
  check_cache(&shares.cache, report);
}

fn check_cache(cache: &ShareCache, report: &mut Report) {
  if let Some(workers) = cache.workers {
    report.check(
      "shares.cache.workers",
      range("shares.cache.workers", workers, 1, MAX_SCAN_WORKERS),
    );
  }
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

/// Says what is wrong with an entry as written, if anything.
fn valid(written: &str) -> Result<(), String> {
  if ROOT.is_match(written) {
    return Err(format!(
      "Share {written} specifies a root mount, which is not supported."
    ));
  }
  let trimmed = trimmed(written);
  if NO_PATH.is_match(trimmed) {
    return Err(format!("Share {written} does not specify a path."));
  }
  let parts = share::entry(trimmed);
  let (alias, path) = (parts.alias, parts.path);
  if !(path.starts_with('/') || path.starts_with("\\\\") || has_drive(path)) {
    return Err(format!(
      "Share {written} contains a relative path. Only absolute paths are supported."
    ));
  }
  if TRAVERSAL.is_match(path) {
    return Err(format!(
      "Share {written} contains an unsafe path traversal segment."
    ));
  }
  if alias.trim().is_empty() {
    return Err(format!(
      "Share '{written}' is invalid. The alias must not be empty or only whitespace."
    ));
  }
  if alias.contains(['/', '\\']) {
    return Err(format!(
      "Share '{written}' is invalid. An alias must not contain the path separators '/' or '\\'."
    ));
  }
  if alias == "." || alias == ".." || alias.chars().any(char::is_control) {
    return Err(format!(
      "Share '{written}' is invalid. The alias must not be '.' or '..' or contain control characters."
    ));
  }
  Ok(())
}

/// A drive letter and a colon, with or without a separator after it.
fn has_drive(path: &str) -> bool {
  let bytes = path.as_bytes();
  bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
  use super::*;

  fn problems_with(directories: &[&str], share_downloads: bool) -> Vec<String> {
    let shares = Shares {
      directories: directories.iter().map(|d| d.to_string()).collect(),
      share_downloads,
      ..Shares::default()
    };
    let mut report = Report::default();
    check(&shares, Path::new("/srv/downloads"), false, &mut report);
    report.messages()
  }

  fn problems(directories: &[&str]) -> Vec<String> {
    problems_with(directories, false)
  }

  #[test]
  fn bad_entries_say_what_is_wrong() {
    assert_eq!(problems(&["[x]"]), ["Share [x] does not specify a path."]);
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
      ["Share music contains a relative path. Only absolute paths are supported."]
    );
    assert_eq!(
      problems(&["/x/.."]),
      ["Share /x/.. contains an unsafe path traversal segment."]
    );
    assert_eq!(
      problems(&["[ ]/music"]),
      ["Share '[ ]/music' is invalid. The alias must not be empty or only whitespace."]
    );
    assert_eq!(
      problems(&["[a\\b]/music"]),
      [
        "Share '[a\\b]/music' is invalid. An alias must not contain the path separators '/' or '\\'."
      ]
    );
    for alias in ["[.]/music", "[..]/music", "[a\u{7}]/music"] {
      assert_eq!(
        problems(&[alias]),
        [format!(
          "Share '{alias}' is invalid. The alias must not be '.' or '..' or contain control characters."
        )]
      );
    }
  }

  #[test]
  fn aliases_and_paths_are_unique() {
    assert_eq!(
      problems(&["/a/music", "/b/music"]),
      [
        "Shares '/a/music' and '/b/music' collide. Specify an alias for one or both to disambiguate \
         them."
      ]
    );
    assert_eq!(
      problems(&["/a/Music", "/b/music"]).len(),
      1,
      "aliases differing only in case collide"
    );
    assert_eq!(
      problems(&["[one]/music", "[two]/music/"]),
      ["Shares '[one]/music' and '[two]/music/' alias the same path."]
    );
    assert!(problems(&["[one]/a/music", "/b/music"]).is_empty());
    assert!(
      problems(&["/a/music", "/a/music/"]).is_empty(),
      "the same entry twice is one share"
    );
  }

  #[test]
  fn the_shared_downloads_directory_is_named_by_its_option() {
    assert!(
      problems_with(&["/srv/downloads/"], true).is_empty(),
      "shared already"
    );
    assert_eq!(
      problems_with(&["/mnt/downloads"], true),
      [
        "Share '/srv/downloads' (from shares.share_downloads) collides with '/mnt/downloads'. \
         Specify an alias for '/mnt/downloads' to disambiguate them."
      ]
    );
    assert_eq!(
      problems_with(&["![dl]/srv/downloads"], true),
      [
        "Share '/srv/downloads' (from shares.share_downloads) aliases the same path as '![dl]/srv/downloads'."
      ]
    );
    assert!(problems_with(&["/mnt/downloads"], false).is_empty());
  }
}
