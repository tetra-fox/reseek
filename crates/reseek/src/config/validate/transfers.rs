//! Uploads, downloads and the user groups: slots, speeds, limits, retries, where downloads go,
//! and the groups' names, patterns and networks.

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use super::Report;
use super::fields::{at_least, cidr, duration_at_least, is_regex, optional_at_least};
use super::paths::{non_traversing, relative_everywhere};
use crate::config::{Destination, GroupUpload, Limits, Transfers};

/// Group names that belong to the built-in groups.
const RESERVED_GROUPS: &[&str] = &["privileged", "default", "leechers", "blacklisted"];

/// A chmod mode in octal.
static MODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-7]{3,4}$").unwrap());

pub(super) fn check(transfers: &Transfers, report: &mut Report) {
  let upload = &transfers.upload;
  report.check(
    "transfers.upload.slots",
    at_least("transfers.upload.slots", upload.slots, 1),
  );
  check_speed("transfers.upload.speed_limit", upload.speed_limit, report);
  check_limits("transfers.upload.limits", &upload.limits, report);

  let download = &transfers.download;
  report.check(
    "transfers.download.slots",
    optional_at_least("transfers.download.slots", download.slots, 1),
  );
  check_speed(
    "transfers.download.speed_limit",
    download.speed_limit,
    report,
  );
  let retry = &download.retry;
  report.check(
    "transfers.download.retry.attempts",
    at_least("transfers.download.retry.attempts", retry.attempts, 1),
  );
  report.check(
    "transfers.download.retry.delay",
    duration_at_least(
      "transfers.download.retry.delay",
      retry.delay,
      Duration::from_secs(1),
    ),
  );
  report.check(
    "transfers.download.retry.max_delay",
    duration_at_least(
      "transfers.download.retry.max_delay",
      retry.max_delay,
      Duration::from_secs(30),
    ),
  );
  check_destination(&download.destination, report);

  let groups = &transfers.groups;
  check_group(
    "transfers.groups.default.upload",
    &groups.default.upload,
    report,
  );
  check_group(
    "transfers.groups.leechers.upload",
    &groups.leechers.upload,
    report,
  );
  let thresholds = &groups.leechers.thresholds;
  for (key, value) in [
    (
      "transfers.groups.leechers.thresholds.files",
      thresholds.files,
    ),
    (
      "transfers.groups.leechers.thresholds.directories",
      thresholds.directories,
    ),
  ] {
    report.check(key, at_least(key, value, 1));
  }
  for pattern in &groups.blacklisted.patterns {
    if !is_regex(pattern) {
      report.add(
        "transfers.groups.blacklisted.patterns",
        format!("Pattern '{pattern}' is not a valid regular expression"),
      );
    }
  }
  for range in &groups.blacklisted.cidrs {
    report.check("transfers.groups.blacklisted.cidrs", cidr(range));
  }
  let key = "transfers.groups.user_defined";
  for (name, group) in &groups.user_defined {
    if RESERVED_GROUPS.contains(&name.as_str()) {
      report.add(
        key,
        format!(
          "User defined group '{name}' collides with a built in group.  Choose a different name."
        ),
      );
    }
    let mut entry = Report::default();
    check_group(&format!("{key}.{name}.upload"), &group.upload, &mut entry);
    for problem in entry.0 {
      report.add(key, problem.message);
    }
  }
}

fn check_speed(key: &str, limit: Option<u32>, report: &mut Report) {
  report.check(key, optional_at_least(key, limit, 1));
}

fn check_limits(prefix: &str, limits: &Limits, report: &mut Report) {
  for (window, limit) in [
    ("queued", &limits.queued),
    ("daily", &limits.daily),
    ("weekly", &limits.weekly),
  ] {
    for (what, value) in [
      ("files", limit.files),
      ("megabytes", limit.megabytes),
      ("failures", limit.failures),
    ] {
      let key = format!("{prefix}.{window}.{what}");
      report.check(&key, optional_at_least(&key, value, 1));
    }
  }
}

/// A group's upload settings, each reported under its own key.
fn check_group(prefix: &str, upload: &GroupUpload, report: &mut Report) {
  let key = format!("{prefix}.priority");
  report.check(&key, at_least(&key, upload.priority, 1));
  let key = format!("{prefix}.slots");
  report.check(&key, optional_at_least(&key, upload.slots, 1));
  check_speed(&format!("{prefix}.speed_limit"), upload.speed_limit, report);
  check_limits(&format!("{prefix}.limits"), &upload.limits, report);
}

fn check_destination(destination: &Destination, report: &mut Report) {
  let key = "transfers.download.destination.subdirectory";
  if let Some(subdirectory) = &destination.subdirectory {
    if subdirectory.is_empty() {
      report.add(
        key,
        format!("The field {key} must contain at least one character"),
      );
    } else if subdirectory.trim().is_empty() {
      report.add(
        key,
        format!("The field {key} must not contain only whitespace"),
      );
    } else {
      report.check(key, relative_everywhere(key, subdirectory));
      report.check(key, non_traversing(key, subdirectory));
    }
  }
  let key = "transfers.download.destination.permissions.mode";
  if let Some(mode) = &destination.permissions.mode
    && !MODE.is_match(mode)
  {
    report.add(
      key,
      format!(
        "The field {key} is invalid. Specify a three- or four-character string consisting of \
         only 0-7 (chmod syntax, [0]000-[7]777, inclusive)"
      ),
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::UserDefinedGroup;

  fn problems(transfers: &Transfers) -> Vec<(String, String)> {
    let mut report = Report::default();
    check(transfers, &mut report);
    report.0.into_iter().map(|i| (i.key, i.message)).collect()
  }

  #[test]
  fn built_in_group_names_are_reserved() {
    let mut transfers = Transfers::default();
    for name in ["blacklisted", "friends"] {
      transfers
        .groups
        .user_defined
        .insert(name.into(), UserDefinedGroup::default());
    }
    assert_eq!(
      problems(&transfers),
      [(
        "transfers.groups.user_defined".to_string(),
        "User defined group 'blacklisted' collides with a built in group.  Choose a different \
         name."
          .to_string()
      )]
    );
  }

  #[test]
  fn a_group_entrys_problem_is_reported_under_the_map() {
    let mut transfers = Transfers::default();
    let mut group = UserDefinedGroup::default();
    group.upload.priority = 0;
    transfers
      .groups
      .user_defined
      .insert("friends".into(), group);
    assert_eq!(
      problems(&transfers),
      [(
        "transfers.groups.user_defined".to_string(),
        "The field transfers.groups.user_defined.friends.upload.priority must be at least 1."
          .to_string()
      )]
    );
  }

  #[test]
  fn the_destination_is_a_safe_relative_template() {
    let mut transfers = Transfers::default();
    assert!(problems(&transfers).is_empty(), "the default template");
    for (subdirectory, expected) in [
      ("", "must contain at least one character"),
      ("  ", "must not contain only whitespace"),
      ("/abs", "must be a relative path on all operating systems."),
      (
        "C:\\abs",
        "must be a relative path on all operating systems.",
      ),
      ("${SOURCE_USERNAME}/..", "unsafe path traversal segments"),
    ] {
      transfers.download.destination.subdirectory = Some(subdirectory.into());
      let found = problems(&transfers);
      assert!(found[0].1.contains(expected), "{subdirectory}: {found:?}");
    }
    transfers.download.destination.subdirectory = None;
    transfers.download.destination.permissions.mode = Some("0644".into());
    assert!(problems(&transfers).is_empty());
    transfers.download.destination.permissions.mode = Some("rw-r--r--".into());
    assert_eq!(problems(&transfers).len(), 1);
  }
}
