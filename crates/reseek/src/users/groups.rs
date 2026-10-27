//! Which upload group a user is in: privileged by the server's word, then a user-defined group
//! the configuration lists them in, then one we put them in, then leechers by their
//! statistics, then the default group.

use api_types::users::{GroupSource, StatisticsSnapshot, UserGroup, UserPresence};

use super::store::Stored;
use crate::config::Options;

/// The group of users the server calls privileged.
pub const PRIVILEGED: &str = "privileged";
/// The group of users who share too little.
pub const LEECHERS: &str = "leechers";
/// The group of everyone else.
pub const DEFAULT: &str = "default";

/// The colour a group is shown in: the built-in groups each their own, every user-defined group
/// the same, the default group none.
fn color(name: &str, source: GroupSource) -> Option<String> {
  let color = match (source, name) {
    (GroupSource::Builtin, PRIVILEGED) => "violet",
    (GroupSource::Builtin, LEECHERS) => "orange",
    (GroupSource::Builtin, _) => return None,
    (GroupSource::Config | GroupSource::Override, _) => "blue",
  };
  Some(color.to_string())
}

/// The user-defined group the configuration lists `username` in, with its priority: of
/// several, the lowest priority number, then the first name.
pub fn configured(options: &Options, username: &str) -> Option<(String, u32)> {
  options
    .transfers
    .groups
    .user_defined
    .iter()
    .filter(|(_, group)| group.members.iter().any(|m| m == username))
    .map(|(name, group)| (name.clone(), group.upload.priority))
    .min_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
}

/// What a user's group is worked out from, besides the configuration, and what else the
/// server last said of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
  /// the server calls them privileged
  pub privileged: bool,
  /// their presence as the server last said, `None` until it said
  pub presence: Option<UserPresence>,
  /// their statistics as the server last said, with when; `None` until it said
  pub statistics: Option<StatisticsSnapshot>,
  /// the user-defined group we put them in
  pub group_override: Option<String>,
}

impl Facts {
  /// The facts a user's stored row holds.
  pub fn of(stored: &Stored) -> Self {
    Self {
      privileged: stored.is_privileged,
      presence: stored.presence.map(|(presence, _)| presence),
      statistics: stored
        .statistics
        .as_ref()
        .map(|(statistics, at)| StatisticsSnapshot {
          statistics: statistics.clone(),
          updated_at: *at,
        }),
      group_override: stored.group_override.clone(),
    }
  }
}

/// The group `username` is in, from what is stored about them. A group we put them in that
/// the configuration no longer has is passed over.
pub fn effective(options: &Options, username: &str, stored: Option<&Stored>) -> UserGroup {
  resolve(
    options,
    username,
    &stored.map(Facts::of).unwrap_or_default(),
  )
}

/// The group `username` is in by `facts`. Shares not known make no leecher.
pub fn resolve(options: &Options, username: &str, facts: &Facts) -> UserGroup {
  let groups = &options.transfers.groups;
  let builtin = |name: &str, priority: u32| UserGroup {
    name: name.to_string(),
    source: GroupSource::Builtin,
    priority,
    color: color(name, GroupSource::Builtin),
  };
  if facts.privileged {
    return builtin(PRIVILEGED, 0);
  }
  if let Some((name, priority)) = configured(options, username) {
    return UserGroup {
      color: color(&name, GroupSource::Config),
      name,
      source: GroupSource::Config,
      priority,
    };
  }
  if let Some(name) = &facts.group_override
    && let Some(group) = groups.user_defined.get(name)
  {
    return UserGroup {
      name: name.clone(),
      source: GroupSource::Override,
      priority: group.upload.priority,
      color: color(name, GroupSource::Override),
    };
  }
  let thresholds = &groups.leechers.thresholds;
  let leeches = facts.statistics.as_ref().is_some_and(|snapshot| {
    let shared = &snapshot.statistics;
    shared.file_count < thresholds.files || shared.directory_count < thresholds.directories
  });
  if leeches {
    return builtin(LEECHERS, groups.leechers.upload.priority);
  }
  builtin(DEFAULT, groups.default.upload.priority)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::UserDefinedGroup;
  use api_types::state::UserStatistics;

  fn stored() -> Stored {
    let now = "2026-10-25T12:00:00Z".parse().unwrap();
    Stored {
      username: "alice".to_string(),
      first_seen_at: now,
      last_seen_at: now,
      presence: None,
      is_privileged: false,
      statistics: None,
      country_code: None,
      info: None,
      notes: String::new(),
      tags: Vec::new(),
      group_override: None,
      muted: false,
      pinned: false,
    }
  }

  fn group(priority: u32, members: &[&str]) -> UserDefinedGroup {
    let mut group = UserDefinedGroup::default();
    group.upload.priority = priority;
    group.members = members.iter().map(|m| m.to_string()).collect();
    group
  }

  #[test]
  fn privilege_then_configuration_then_override_then_statistics() {
    let mut options = Options::default();
    let groups = &mut options.transfers.groups.user_defined;
    groups.insert("friends".into(), group(5, &["alice"]));
    groups.insert("family".into(), group(3, &["alice"]));
    groups.insert("crew".into(), group(3, &["alice"]));
    groups.insert("vip".into(), group(2, &[]));
    let mut alice = stored();
    alice.group_override = Some("vip".to_string());
    let name = |options: &Options, s: &Stored| effective(options, "alice", Some(s));
    let picked = name(&options, &alice);
    assert_eq!(
      (picked.name.as_str(), picked.source),
      ("crew", GroupSource::Config),
      "the lowest priority number, ties by name, and over the override"
    );
    assert_eq!(picked.color.as_deref(), Some("blue"));
    alice.is_privileged = true;
    let privileged = name(&options, &alice);
    assert_eq!(privileged.name, PRIVILEGED);
    assert_eq!(privileged.color.as_deref(), Some("violet"));
    alice.is_privileged = false;
    for g in options.transfers.groups.user_defined.values_mut() {
      g.members.clear();
    }
    assert_eq!(name(&options, &alice).source, GroupSource::Override);
    options.transfers.groups.user_defined.remove("vip");
    alice.statistics = Some((
      UserStatistics {
        average_speed: 0,
        directory_count: 4,
        file_count: 0,
        upload_count: 0,
      },
      alice.last_seen_at,
    ));
    assert_eq!(
      name(&options, &alice).name,
      LEECHERS,
      "an override of a group gone is passed over"
    );
    alice.statistics = None;
    assert_eq!(
      name(&options, &alice).name,
      DEFAULT,
      "unknown is no leecher"
    );
    assert_eq!(name(&options, &alice).color, None);
    assert_eq!(effective(&options, "nobody", None).name, DEFAULT);
  }
}
