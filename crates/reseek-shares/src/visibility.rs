//! Who sees which shares. A share is visible to everyone unless the share options list its
//! alias with the groups whose members see it. Every answer to another user, their browse, a
//! folder's contents, a search and a file they ask for, goes through the [`View`] of what they
//! see, so the answers agree: a share hidden from a user is answered as one we do not have.

use std::collections::BTreeSet;

use sha1::{Digest, Sha1};

use crate::options::Shares;

/// The groups the share options let see the share `alias`, matched ignoring case: `None` for a
/// share everyone sees, empty for one listed with no group, which no one sees.
pub fn of(options: &Shares, alias: &str) -> Option<Vec<String>> {
  options
    .visible_to
    .iter()
    .find(|(listed, _)| listed.to_lowercase() == alias.to_lowercase())
    .map(|(_, groups)| groups.clone())
}

/// What one user sees of our shares: every share but those it hides.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct View {
  /// the aliases of the shares hidden, lower case, sorted and each once
  hidden: Vec<String>,
}

impl View {
  /// Every share, hiding none: what the local user sees.
  pub fn everything() -> Self {
    Self::default()
  }

  /// What a member of no group sees under `options`: the shares everyone sees.
  pub fn everyone(options: &Shares) -> Self {
    Self::of(options, &BTreeSet::new())
  }

  /// What a member of `groups` sees under `options`: every share everyone sees, and each one
  /// listed for one of `groups`.
  pub fn of(options: &Shares, groups: &BTreeSet<String>) -> Self {
    let mut hidden: Vec<String> = options
      .visible_to
      .iter()
      .filter(|(_, seen_by)| !seen_by.iter().any(|group| groups.contains(group)))
      .map(|(alias, _)| alias.to_lowercase())
      .collect();
    hidden.sort();
    hidden.dedup();
    Self { hidden }
  }

  /// Whether the view hides no share.
  pub fn is_everything(&self) -> bool {
    self.hidden.is_empty()
  }

  /// Whether the view hides the share `alias`.
  pub fn hides_share(&self, alias: &str) -> bool {
    !self.hidden.is_empty() && self.hidden.contains(&alias.to_lowercase())
  }

  /// Whether the view hides the directory or file with the remote name `name`, which starts
  /// with its share's alias.
  pub fn hides(&self, name: &str) -> bool {
    !self.hidden.is_empty() && self.hides_share(name.split('\\').next().unwrap_or(name))
  }

  /// A short name for the view that differs between views, for the files kept per view.
  pub(crate) fn key(&self) -> String {
    let digest = Sha1::digest(self.hidden.join("\0").as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_member_of_any_listed_group_sees_a_share() {
    let options = Shares {
      visible_to: [
        ("Rare".to_string(), vec!["buddies".to_string()]),
        ("Gone".to_string(), Vec::new()),
        (
          "private".to_string(),
          vec!["close-friends".to_string(), "family".to_string()],
        ),
      ]
      .into(),
      ..Shares::default()
    };
    let groups = |names: &[&str]| names.iter().map(|n| n.to_string()).collect();
    let everyone = View::of(&options, &groups(&[]));
    assert!(everyone.hides("rare\\a.flac"), "matched ignoring case");
    assert!(everyone.hides("Private"));
    assert!(
      !everyone.hides("Music\\Rare\\a.flac"),
      "only the alias counts"
    );
    let buddies = View::of(&options, &groups(&["buddies"]));
    assert!(!buddies.hides("Rare\\a.flac"));
    assert!(buddies.hides("Private\\a.flac"));
    let family = View::of(&options, &groups(&["family"]));
    assert!(family.hides("Rare\\a.flac"), "no wider audience is implied");
    assert!(!family.hides("Private\\a.flac"));
    let both = View::of(&options, &groups(&["buddies", "family"]));
    assert!(!both.hides("Private") && !both.hides("Rare"));
    assert!(
      both.hides("Gone"),
      "a share listed with no group is seen by no one"
    );
    assert_ne!(everyone.key(), buddies.key());
    assert_eq!(
      of(&options, "PRIVATE").unwrap(),
      ["close-friends", "family"]
    );
    assert_eq!(of(&options, "gone"), Some(Vec::new()));
    assert_eq!(of(&options, "Music"), None);
  }
}
