//! Which groups a user is in, and what their uploads get from them. The smart groups take their
//! members from what the server says: privileged by its word, leechers by their statistics.
//! The manual ones are ours: buddies is the buddy list, and each user-defined group of the
//! configuration holds the users we put in it. Default is everyone. Each upload setting comes
//! from the highest group in the configured order the user is in that sets it, else from
//! default: the queue they wait in (a group setting slots or a queue order), the speed limit
//! they count against, and each per-user limit. Privileged users go before everyone.

use std::collections::BTreeSet;

use api_types::users::{
  GroupKind, GroupLimit, LimitMeasure, LimitWindow, StatisticsSnapshot, UserGroup, UserPresence,
  UserUploads,
};

use super::store::Stored;
use crate::config::{
  BUDDIES_GROUP, BUILTIN_GROUPS, DEFAULT_GROUP, GroupUpload, LEECHERS_GROUP, Limits, Options,
  PRIVILEGED_GROUP,
};

/// What a user's groups are worked out from, besides the configuration and the buddy list, and
/// what else the server last said of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
  /// the server calls them privileged
  pub privileged: bool,
  /// their presence as the server last said, `None` until it said
  pub presence: Option<UserPresence>,
  /// their statistics as the server last said, with when; `None` until it said
  pub statistics: Option<StatisticsSnapshot>,
  /// the groups we put them in, by name
  pub memberships: Vec<String>,
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
      memberships: stored.groups.clone(),
    }
  }

  /// The server said something of the user in this run, so their groups are worked out from
  /// these facts rather than their row.
  pub fn heard(&self) -> bool {
    self.privileged || self.presence.is_some() || self.statistics.is_some()
  }
}

/// The groups a user is in, and what their uploads get from them.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
  /// every group they are in but default, as [`place`] orders them
  pub groups: Vec<UserGroup>,
  /// the server calls them privileged, so they go before everyone without per-user limits
  pub privileged: bool,
  /// the group whose queue they wait in
  pub queue: String,
  /// the group whose speed limit their uploads count against
  pub speed: String,
  /// the per-user limits their groups set, each field the highest group's that sets it; a field
  /// none sets is taken from the global limits
  pub limits: Limits,
  /// the group that set each of `limits`' fields, in [`LIMIT_FIELDS`] order
  limit_groups: Vec<Option<String>>,
}

impl Placement {
  /// The groups that decide which shares the user sees: their manual groups the configuration
  /// has.
  pub fn share_groups(&self) -> BTreeSet<String> {
    self
      .groups
      .iter()
      .filter(|g| g.kind == GroupKind::Manual && g.defined)
      .map(|g| g.name.clone())
      .collect()
  }

  /// Whether the user is in the group `name`; everyone is in default.
  pub fn is_in(&self, name: &str) -> bool {
    name == DEFAULT_GROUP || self.groups.iter().any(|g| g.name == name)
  }

  /// What their uploads get, as the API shows it, with the alternative speed limits in force
  /// when `alternative`. A privileged user's uploads go at the total upload limit in force, and
  /// they have no per-user limits.
  pub fn uploads(&self, options: &Options, alternative: bool) -> UserUploads {
    let groups = &options.transfers.groups;
    let global = options.transfers.upload.slots;
    let slots = groups
      .upload(&self.queue)
      .and_then(|upload| upload.slots)
      .filter(|&slots| slots != 0 && slots < global);
    let speed_limit = match self.speed.as_str() {
      PRIVILEGED_GROUP => options.transfers.upload_speed_limit(alternative),
      name => groups.upload(name).and_then(|upload| upload.speed_limit),
    }
    .filter(|&limit| limit != 0);
    let limits = LIMIT_FIELDS
      .iter()
      .filter(|_| !self.privileged)
      .zip(&self.limit_groups)
      .filter_map(|(field, group)| {
        Some(GroupLimit {
          window: field.window,
          measure: field.measure,
          limit: (field.get)(&self.limits).filter(|&limit| limit != 0),
          group: group.clone()?,
        })
      })
      .collect();
    UserUploads {
      queue: self.queue.clone(),
      slots: if self.privileged { None } else { slots },
      speed: self.speed.clone(),
      speed_limit,
      limits,
    }
  }
}

/// One field of the per-user limits: which window and measure it is, and how to read and set
/// it.
struct LimitField {
  window: LimitWindow,
  measure: LimitMeasure,
  get: fn(&Limits) -> Option<u32>,
  set: fn(&mut Limits, Option<u32>),
}

macro_rules! limit_field {
  ($window:ident, $measure:ident, $w:ident . $m:ident) => {
    LimitField {
      window: LimitWindow::$window,
      measure: LimitMeasure::$measure,
      get: |limits| limits.$w.$m,
      set: |limits, value| limits.$w.$m = value,
    }
  };
}

/// Every field of the per-user limits.
const LIMIT_FIELDS: [LimitField; 9] = [
  limit_field!(Queued, Files, queued.files),
  limit_field!(Queued, Megabytes, queued.megabytes),
  limit_field!(Queued, Failures, queued.failures),
  limit_field!(Daily, Files, daily.files),
  limit_field!(Daily, Megabytes, daily.megabytes),
  limit_field!(Daily, Failures, daily.failures),
  limit_field!(Weekly, Files, weekly.files),
  limit_field!(Weekly, Megabytes, weekly.megabytes),
  limit_field!(Weekly, Failures, weekly.failures),
];

/// Whether the server's word decides who is in the group `name`: the smart groups but default,
/// which holds everyone.
pub fn decided_by_server(name: &str) -> bool {
  name == PRIVILEGED_GROUP || name == LEECHERS_GROUP
}

/// The group `name` as the API shows it: a built-in group, a user-defined group of `options`,
/// or the name of one it does not have.
pub fn describe(options: &Options, name: &str) -> UserGroup {
  let groups = &options.transfers.groups;
  let (kind, defined, color) = match name {
    PRIVILEGED_GROUP => (GroupKind::Smart, true, Some("violet")),
    LEECHERS_GROUP => (GroupKind::Smart, true, Some("orange")),
    DEFAULT_GROUP => (GroupKind::Smart, true, None),
    BUDDIES_GROUP => (GroupKind::Manual, true, Some("green")),
    name => match groups.user_defined.get(name) {
      Some(group) => (GroupKind::Manual, true, Some(group.color.as_str())),
      None => (GroupKind::Manual, false, None),
    },
  };
  UserGroup {
    name: name.to_string(),
    kind,
    defined,
    upload_settings: groups.upload(name).is_some_and(GroupUpload::sets_any),
    color: color.map(str::to_string),
  }
}

/// A group we put a user in. One named like a built-in group, which the API refuses but an
/// older tag may be, is a name the configuration does not have.
fn membership(options: &Options, name: &str) -> UserGroup {
  if BUILTIN_GROUPS.contains(&name) {
    return UserGroup {
      name: name.to_string(),
      kind: GroupKind::Manual,
      defined: false,
      upload_settings: false,
      color: None,
    };
  }
  describe(options, name)
}

/// Whether a user with `statistics` shares too little under `options`. Shares not known make
/// no leecher.
fn leeches(options: &Options, statistics: Option<&StatisticsSnapshot>) -> bool {
  let thresholds = &options.transfers.groups.leechers.thresholds;
  statistics.is_some_and(|snapshot| {
    let shared = &snapshot.statistics;
    shared.file_count < thresholds.files || shared.directory_count < thresholds.directories
  })
}

/// Whether a user's facts going from `before` to `after` move the groups they are shown in under
/// `options`: the first word on them in this run, from which their groups are worked out from
/// the facts rather than their row, or a change of privileges or of whether they share too
/// little.
pub fn moves(options: &Options, before: &Facts, after: &Facts) -> bool {
  (!before.heard() && after.heard())
    || before.privileged != after.privileged
    || leeches(options, before.statistics.as_ref()) != leeches(options, after.statistics.as_ref())
}

/// The groups of a user with `facts`, our buddy when `buddy`, under `options`: privileged,
/// leechers and buddies when they are in them, then the groups we put them in by name.
pub fn place(options: &Options, facts: &Facts, buddy: bool) -> Placement {
  let privileged = describe(options, PRIVILEGED_GROUP);
  let leechers = describe(options, LEECHERS_GROUP);
  let leeches = leeches(options, facts.statistics.as_ref());
  let mut groups = Vec::new();
  if facts.privileged {
    groups.push(privileged.clone());
  }
  if leeches {
    groups.push(leechers.clone());
  }
  if buddy {
    groups.push(describe(options, BUDDIES_GROUP));
  }
  let mut memberships: Vec<&String> = facts.memberships.iter().collect();
  memberships.sort();
  memberships.dedup();
  groups.extend(
    memberships
      .into_iter()
      .map(|name| membership(options, name)),
  );
  // the groups whose settings count, highest first, then default
  let config = &options.transfers.groups;
  let chain: Vec<(&str, &GroupUpload)> = config
    .ordered()
    .into_iter()
    .filter(|name| groups.iter().any(|g| g.name == *name && g.defined))
    .chain([DEFAULT_GROUP])
    .filter_map(|name| Some((name, config.upload(name)?)))
    .collect();
  let first = |sets: fn(&GroupUpload) -> bool| {
    chain
      .iter()
      .find(|(_, upload)| sets(upload))
      .map_or(DEFAULT_GROUP, |(name, _)| *name)
      .to_string()
  };
  let (queue, speed) = if facts.privileged {
    (PRIVILEGED_GROUP.to_string(), PRIVILEGED_GROUP.to_string())
  } else {
    (
      first(GroupUpload::own_queue),
      first(|upload| upload.speed_limit.is_some()),
    )
  };
  let mut limits = Limits::default();
  let mut limit_groups = Vec::new();
  for field in &LIMIT_FIELDS {
    let set = chain
      .iter()
      .find_map(|(name, upload)| Some((*name, (field.get)(&upload.limits)?)));
    (field.set)(&mut limits, set.map(|(_, value)| value));
    limit_groups.push(set.map(|(name, _)| name.to_string()));
  }
  Placement {
    groups,
    privileged: facts.privileged,
    queue,
    speed,
    limits,
    limit_groups,
  }
}

/// Each share alias of `shares.visible_to` with a group listed for it that no member can see
/// it through: one that is neither buddies nor a user-defined group, such as a misspelled name
/// or a smart group.
pub fn unknown_share_groups(options: &Options) -> BTreeSet<(String, String)> {
  let groups = &options.transfers.groups;
  options
    .shares
    .visible_to
    .iter()
    .flat_map(|(alias, seen_by)| seen_by.iter().map(move |group| (alias, group)))
    .filter(|(_, group)| *group != BUDDIES_GROUP && !groups.user_defined.contains_key(*group))
    .map(|(alias, group)| (alias.clone(), group.clone()))
    .collect()
}

/// Each group of `transfers.download.received.from` no user can be in, such as a misspelled
/// name: one that is neither everyone, buddies, a smart group nor a user-defined group.
pub fn unknown_sender_groups(options: &Options) -> BTreeSet<String> {
  let known = [
    DEFAULT_GROUP,
    BUDDIES_GROUP,
    LEECHERS_GROUP,
    PRIVILEGED_GROUP,
  ];
  options
    .transfers
    .download
    .received
    .from
    .iter()
    .filter(|group| {
      !known.contains(&group.as_str())
        && !options.transfers.groups.user_defined.contains_key(*group)
    })
    .cloned()
    .collect()
}

/// Who our user info tells peers may send us files unasked, as the peer protocol counts it: 0 no
/// one, 1 everyone, 2 our buddies, 3 some users we permit, for groups of
/// `transfers.download.received.from` other than buddies. A group no one can be in counts as
/// none.
pub fn upload_permitted(options: &Options) -> u32 {
  let unknown = unknown_sender_groups(options);
  let groups: Vec<&str> = options
    .transfers
    .download
    .received
    .from
    .iter()
    .filter(|group| !unknown.contains(*group))
    .map(String::as_str)
    .collect();
  if groups.is_empty() {
    0
  } else if groups.contains(&DEFAULT_GROUP) {
    1
  } else if groups.iter().all(|group| *group == BUDDIES_GROUP) {
    2
  } else {
    3
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::UserDefinedGroup;
  use api_types::state::UserStatistics;
  use chrono::Utc;

  fn sharing(files: u32) -> Option<StatisticsSnapshot> {
    Some(StatisticsSnapshot {
      statistics: UserStatistics {
        average_speed: 0,
        directory_count: 4,
        file_count: files,
        upload_count: 0,
      },
      updated_at: Utc::now(),
    })
  }

  fn names(placement: &Placement) -> Vec<&str> {
    placement.groups.iter().map(|g| g.name.as_str()).collect()
  }

  fn order(options: &mut Options, names: &[&str]) {
    options.transfers.groups.order = names.iter().map(|n| n.to_string()).collect();
  }

  /// Options where the leechers get 1 slot, 32 KiB/s and 10 files a day, `throttled` sets 50
  /// KiB/s and 1 slot, and `jazz` sets nothing.
  fn options() -> Options {
    let mut options = Options::default();
    let groups = &mut options.transfers.groups;
    groups.leechers.upload = GroupUpload {
      slots: Some(1),
      speed_limit: Some(32),
      ..GroupUpload::default()
    };
    groups.leechers.upload.limits.daily.files = Some(10);
    let throttled = GroupUpload {
      slots: Some(1),
      speed_limit: Some(50),
      ..GroupUpload::default()
    };
    groups.user_defined.insert(
      "throttled".into(),
      UserDefinedGroup {
        upload: throttled,
        ..UserDefinedGroup::default()
      },
    );
    groups
      .user_defined
      .insert("jazz".into(), UserDefinedGroup::default());
    options
  }

  #[test]
  fn each_setting_comes_from_the_highest_group_that_sets_it() {
    let mut options = options();
    let mut facts = Facts {
      statistics: sharing(0),
      memberships: ["jazz", "gone"].map(String::from).to_vec(),
      ..Facts::default()
    };
    let leecher = place(&options, &facts, false);
    assert_eq!(names(&leecher), ["leechers", "gone", "jazz"]);
    assert_eq!(
      (leecher.queue.as_str(), leecher.speed.as_str()),
      ("leechers", "leechers"),
      "jazz sets nothing, so it changes nothing"
    );
    assert_eq!(leecher.limits.daily.files, Some(10));
    assert_eq!(
      leecher.share_groups(),
      ["jazz"].map(String::from).into(),
      "a group the configuration lacks gives nothing"
    );

    let buddy = place(&options, &facts, true);
    assert_eq!(
      (buddy.queue.as_str(), buddy.speed.as_str()),
      ("buddies", "buddies"),
      "the buddies' own slots and speed are above the leechers'"
    );
    let uploads = buddy.uploads(&options, false);
    assert_eq!(uploads.speed_limit, None, "the buddies' 0 is no limit");
    assert_eq!(uploads.slots, None, "and every slot");
    assert_eq!(
      uploads.limits,
      [GroupLimit {
        window: LimitWindow::Daily,
        measure: LimitMeasure::Files,
        limit: Some(10),
        group: "leechers".into(),
      }],
      "the buddies set no daily files, so the leechers' still apply"
    );

    facts.memberships = vec!["throttled".into()];
    let below = place(&options, &facts, true);
    assert_eq!(
      below.speed, "buddies",
      "a group left out of the order goes just above default"
    );
    order(&mut options, &["throttled", "buddies", "leechers"]);
    let above = place(&options, &facts, true);
    assert_eq!(
      (above.queue.as_str(), above.speed.as_str()),
      ("throttled", "throttled")
    );
    assert_eq!(above.uploads(&options, false).speed_limit, Some(50));

    facts.privileged = true;
    let privileged = place(&options, &facts, true);
    assert_eq!(
      (privileged.queue.as_str(), privileged.speed.as_str()),
      ("privileged", "privileged")
    );
    assert!(
      privileged.uploads(&options, false).limits.is_empty(),
      "the leechers' daily limit does not apply to a privileged leecher"
    );
    options.transfers.upload.speed_limit = Some(500);
    options.transfers.upload.alt_speed_limit = Some(50);
    let speed = |alternative| privileged.uploads(&options, alternative).speed_limit;
    assert_eq!(
      (speed(false), speed(true)),
      (Some(500), Some(50)),
      "the total upload limit in force"
    );
  }

  #[test]
  fn a_share_listed_for_a_group_no_member_can_see_it_through_is_found() {
    let mut options = options();
    options.shares.visible_to = [
      (
        "Rare".to_string(),
        ["buddies", "jazz", "Jazz", "leechers"]
          .map(String::from)
          .to_vec(),
      ),
      ("Live".to_string(), Vec::new()),
    ]
    .into();
    let pair = |alias: &str, group: &str| (alias.to_string(), group.to_string());
    assert_eq!(
      unknown_share_groups(&options),
      [pair("Rare", "Jazz"), pair("Rare", "leechers")].into()
    );
  }

  #[test]
  fn a_group_no_one_can_be_in_is_found_among_those_who_may_send_files() {
    let mut options = options();
    options.transfers.download.received.from = [
      "default",
      "buddies",
      "leechers",
      "privileged",
      "jazz",
      "budies",
      "blacklisted",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(
      unknown_sender_groups(&options),
      ["budies", "blacklisted"].map(String::from).into()
    );
  }

  #[test]
  fn everyone_holds_what_no_group_sets_and_built_in_names_are_never_ours() {
    let mut options = Options::default();
    options.transfers.groups.default.upload.speed_limit = Some(100);
    let facts = Facts {
      memberships: ["buddies", "privileged"].map(String::from).to_vec(),
      ..Facts::default()
    };
    let placed = place(&options, &facts, false);
    assert_eq!(
      (placed.queue.as_str(), placed.speed.as_str()),
      ("default", "default")
    );
    assert_eq!(placed.uploads(&options, false).speed_limit, Some(100));
    assert!(placed.uploads(&options, false).limits.is_empty());
    assert!(placed.groups.iter().all(|g| !g.defined));
    assert!(placed.share_groups().is_empty());
    assert!(placed.is_in("default"), "everyone is in default");
    let enough = Facts {
      statistics: sharing(10),
      ..Facts::default()
    };
    assert!(!place(&options, &enough, false).is_in("leechers"));
  }
}
