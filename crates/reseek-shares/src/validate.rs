//! What the share options must satisfy: each shared directory's grammar, unique aliases and
//! paths, filters that compile, no share both buddy-only and trusted-only, and the index's
//! workers and rescan interval in range.
//!
//! An entry is an optional `!` or `-` that excludes it, an optional `[alias]`, and an absolute
//! path. Without an alias the alias is the path's last segment. The first `]` ends the alias,
//! so a path may itself hold `]`.

use std::path::Path;
use std::sync::LazyLock;

use cascade::Invalid;
use cascade::checks::range;
use regex::Regex;

use crate::options::{MAX_SCAN_WORKERS, ShareCache, Shares};
use crate::patterns;
use crate::share::{self, DERIVED_FROM_DOWNLOADS, Share, trimmed};

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

/// Every problem with the share options `shares`, under the keys of a `[shares]` section: the
/// shared directories with the share of the downloads directory `downloads` when
/// `shares.share_downloads` adds it, the filters compiled matching case when `case_sensitive`,
/// and the cache options.
pub fn check(shares: &Shares, downloads: &Path, case_sensitive: bool) -> Vec<Invalid> {
  let mut report = Vec::new();
  let mut invalid: Vec<&str> = Vec::new();
  for written in &shares.directories {
    if let Err(message) = valid(written) {
      report.push(Invalid::new(KEY, message));
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
        report.push(Invalid::new(KEY, message));
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
        report.push(Invalid::new(KEY, message));
      }
    }
  }
  for filter in &shares.filters {
    if patterns::compile(filter, case_sensitive).is_err() {
      report.push(Invalid::new(
        "shares.filters",
        format!("Share filter '{filter}' is not a valid regular expression."),
      ));
    }
  }
  for alias in &shares.trusted_only {
    if shares
      .buddies_only
      .iter()
      .any(|other| other.to_lowercase() == alias.to_lowercase())
    {
      report.push(Invalid::new(
        "shares.trusted_only",
        format!("Share '{alias}' is listed as both buddy-only and trusted-only. Keep one."),
      ));
    }
  }
  check_cache(&shares.cache, &mut report);
  report
}

fn check_cache(cache: &ShareCache, report: &mut Vec<Invalid>) {
  let key = "shares.cache.workers";
  if let Some(workers) = cache.workers
    && let Err(message) = range(key, workers, 1, MAX_SCAN_WORKERS)
  {
    report.push(Invalid::new(key, message));
  }
  let key = "shares.cache.retention";
  if let Err(message) = cache.retention.check(std::time::Duration::from_secs(3600)) {
    report.push(Invalid::new(key, message));
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
    check(&shares, Path::new("/srv/downloads"), false)
      .into_iter()
      .map(|invalid| invalid.message)
      .collect()
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
