//! Uploads, downloads and the user groups: slots, speeds, retries, where downloads go,
//! and the groups' names, patterns and networks.

use std::time::Duration;

use super::Report;
use super::fields::{at_least, cidr, duration_at_least, is_regex, optional_at_least};
use crate::config::{
  BUDDIES_GROUP, BUILTIN_GROUPS, DEFAULT_GROUP, Destination, Groups, LEECHERS_GROUP,
  PRIVILEGED_GROUP, Transfers, is_group_name,
};
use crate::files::mode::to_unix_file_mode;
use crate::safety::{non_traversing, relative_everywhere};

pub(super) fn check(transfers: &Transfers, case_sensitive: bool, report: &mut Report) {
  let upload = &transfers.upload;
  report.check(
    "transfers.upload.slots",
    at_least("transfers.upload.slots", upload.slots, 1),
  );
  check_speed("transfers.upload.speed_limit", upload.speed_limit, report);
  check_speed(
    "transfers.upload.alt_speed_limit",
    upload.alt_speed_limit,
    report,
  );

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
  check_speed(
    "transfers.download.alt_speed_limit",
    download.alt_speed_limit,
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
  for pattern in &download.filters {
    if !is_regex(pattern, false) {
      report.add(
        "transfers.download.filters",
        format!("The download filter {pattern} is not a valid regular expression."),
      );
    }
  }

  let groups = &transfers.groups;
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
    if !is_regex(pattern, case_sensitive) {
      report.add(
        "transfers.groups.blacklisted.patterns",
        format!("The pattern '{pattern}' is not a valid regular expression."),
      );
    }
  }
  for range in &groups.blacklisted.cidrs {
    report.check("transfers.groups.blacklisted.cidrs", cidr(range));
  }
  check_order(groups, report);
  let key = "transfers.groups.user_defined";
  for name in groups.user_defined.keys() {
    if BUILTIN_GROUPS.contains(&name.as_str()) {
      report.add(
        key,
        format!(
          "The user-defined group '{name}' collides with a built-in group. Choose a different name."
        ),
      );
    } else if !is_group_name(name) {
      report.add(
        key,
        format!("The group name '{name}' must be 1 to 32 characters from a-z, 0-9, _ and -."),
      );
    }
  }
}

/// A speed limit, when set, is at least 1 KiB/s; no limit is the option left unset.
pub(super) fn check_speed(key: &str, limit: Option<u32>, report: &mut Report) {
  if limit == Some(0) {
    report.add(
      key,
      format!("The field {key} must be at least 1 KiB/s. Leave it unset for no limit."),
    );
  }
}

/// The order holds buddies and leechers, each group once, and neither default, always last,
/// nor privileged, always first. A name no group has is passed over, so a group and the order
/// can be changed in separate files one after the other.
fn check_order(groups: &Groups, report: &mut Report) {
  let key = "transfers.groups.order";
  for (at, name) in groups.order.iter().enumerate() {
    if name == DEFAULT_GROUP {
      report.add(
        key,
        format!("The group '{name}' always goes last; leave it out of {key}."),
      );
    } else if name == PRIVILEGED_GROUP {
      report.add(
        key,
        format!("The group '{name}' always goes first; leave it out of {key}."),
      );
    } else if groups.order[..at].contains(name) {
      report.add(key, format!("The group '{name}' is listed twice in {key}."));
    }
  }
  for name in [BUDDIES_GROUP, LEECHERS_GROUP] {
    if !groups.order.iter().any(|listed| listed == name) {
      report.add(
        key,
        format!("The field {key} must list the group '{name}'."),
      );
    }
  }
}

fn check_destination(destination: &Destination, report: &mut Report) {
  let key = "transfers.download.destination.subdirectory";
  if let Some(subdirectory) = &destination.subdirectory {
    if subdirectory.is_empty() {
      report.add(
        key,
        format!("The field {key} must contain at least one character."),
      );
    } else if subdirectory.trim().is_empty() {
      report.add(
        key,
        format!("The field {key} must not contain only whitespace."),
      );
    } else {
      report.check(key, relative_everywhere(key, subdirectory));
      report.check(key, non_traversing(key, subdirectory));
    }
  }
  let key = "transfers.download.destination.permissions.mode";
  if let Some(mode) = &destination.permissions.mode
    && to_unix_file_mode(mode).is_err()
  {
    report.add(
      key,
      format!(
        "The field {key} is invalid. Specify a three- or four-character string consisting of \
         only 0-7 (chmod syntax, [0]000-[7]777, inclusive)."
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
    check(transfers, false, &mut report);
    report.0.into_iter().map(|i| (i.key, i.message)).collect()
  }

  #[test]
  fn a_speed_limit_of_zero_is_refused_and_unset_is_no_limit() {
    let mut transfers = Transfers::default();
    assert!(problems(&transfers).is_empty());
    transfers.upload.speed_limit = Some(0);
    transfers.download.speed_limit = Some(1);
    assert_eq!(
      problems(&transfers),
      [(
        "transfers.upload.speed_limit".to_string(),
        "The field transfers.upload.speed_limit must be at least 1 KiB/s. Leave it unset for no \
         limit."
          .to_string()
      )]
    );
  }

  #[test]
  fn group_names_are_words_and_built_in_ones_are_reserved() {
    let mut transfers = Transfers::default();
    for name in ["buddies", "friends", "Close friends"] {
      transfers
        .groups
        .user_defined
        .insert(name.into(), UserDefinedGroup::default());
    }
    assert_eq!(
      problems(&transfers),
      [
        (
          "transfers.groups.user_defined".to_string(),
          "The group name 'Close friends' must be 1 to 32 characters from a-z, 0-9, _ and -."
            .to_string()
        ),
        (
          "transfers.groups.user_defined".to_string(),
          "The user-defined group 'buddies' collides with a built-in group. Choose a \
           different name."
            .to_string()
        )
      ]
    );
  }

  #[test]
  fn the_order_lists_the_built_in_groups_and_known_ones_once() {
    let mut transfers = Transfers::default();
    assert!(problems(&transfers).is_empty(), "the default order");
    transfers
      .groups
      .user_defined
      .insert("friends".into(), UserDefinedGroup::default());
    transfers.groups.order = ["friends", "default", "friends", "gone", "leechers"]
      .map(String::from)
      .into();
    let messages: Vec<String> = problems(&transfers).into_iter().map(|(_, m)| m).collect();
    assert_eq!(
      messages,
      [
        "The group 'default' always goes last; leave it out of transfers.groups.order.",
        "The group 'friends' is listed twice in transfers.groups.order.",
        "The field transfers.groups.order must list the group 'buddies'.",
      ],
      "a name no group has is passed over"
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
