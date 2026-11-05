//! Who sees which shares. A share is visible to everyone unless the share options list its
//! alias as buddy-only or trusted-only. Every answer to another user, their browse, a folder's
//! contents, a search and a file they ask for, goes through the [`View`] of what they see, so
//! the answers agree: a share hidden from a user is answered as one we do not have.

use api_types::shares::ShareVisibility;
use sha1::{Digest, Sha1};

use crate::options::Shares;

/// The visibility the share options give the share `alias`, matched ignoring case.
pub fn of(options: &Shares, alias: &str) -> ShareVisibility {
  let listed = |list: &[String]| {
    list
      .iter()
      .any(|a| a.to_lowercase() == alias.to_lowercase())
  };
  if listed(&options.trusted_only) {
    ShareVisibility::Trusted
  } else if listed(&options.buddies_only) {
    ShareVisibility::Buddies
  } else {
    ShareVisibility::Everyone
  }
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

  /// What a user who sees the shares visible to `sees` and every wider audience sees under
  /// `options`.
  pub fn of(options: &Shares, sees: ShareVisibility) -> Self {
    let mut hidden: Vec<String> = Vec::new();
    for (list, visibility) in [
      (&options.buddies_only, ShareVisibility::Buddies),
      (&options.trusted_only, ShareVisibility::Trusted),
    ] {
      if visibility > sees {
        hidden.extend(list.iter().map(|alias| alias.to_lowercase()));
      }
    }
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
  fn each_audience_sees_its_shares_and_every_wider_one() {
    let options = Shares {
      buddies_only: vec!["Rare".into()],
      trusted_only: vec!["private".into()],
      ..Shares::default()
    };
    let everyone = View::of(&options, ShareVisibility::Everyone);
    assert!(everyone.hides("rare\\a.flac"), "matched ignoring case");
    assert!(everyone.hides("Private"));
    assert!(
      !everyone.hides("Music\\Rare\\a.flac"),
      "only the alias counts"
    );
    let buddies = View::of(&options, ShareVisibility::Buddies);
    assert!(!buddies.hides("Rare\\a.flac"));
    assert!(buddies.hides("Private\\a.flac"));
    assert!(View::of(&options, ShareVisibility::Trusted).is_everything());
    assert_ne!(everyone.key(), buddies.key());
    assert_eq!(of(&options, "PRIVATE"), ShareVisibility::Trusted);
    assert_eq!(of(&options, "Music"), ShareVisibility::Everyone);
  }
}
