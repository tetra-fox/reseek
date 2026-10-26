//! The facts each user's upload group is worked out from, kept in memory so the group is known
//! at once wherever it is asked: whether the server calls the user privileged and what they
//! share, as the server last said, and the group we put them in. A user enters on a fetch, a
//! watch or the server's list of privileged users, and every user we put in a group is loaded
//! at startup; nobody leaves. The end of a server session forgets which users the server was
//! watching for us, not what it said of them.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, PoisonError, RwLock};

use soulseek_client::Client;
use soulseek_proto::server::UserStats;
use tracing::{debug, trace, warn};

use super::Directory;
use super::groups::{self, Facts, Shared};

/// The facts of every user known, and the users the server watches for us.
#[derive(Default)]
pub struct GroupCache {
  facts: RwLock<HashMap<String, Facts>>,
  watched: Mutex<HashSet<String>>,
}

impl GroupCache {
  /// The cache knowing the groups `overrides` puts users in.
  pub fn new(overrides: impl IntoIterator<Item = (String, String)>) -> Self {
    let facts = overrides
      .into_iter()
      .map(|(username, group)| {
        let facts = Facts {
          group_override: Some(group),
          ..Facts::default()
        };
        (username, facts)
      })
      .collect();
    Self {
      facts: RwLock::new(facts),
      watched: Mutex::default(),
    }
  }

  /// What is known of `username`, when anything is.
  pub fn facts(&self, username: &str) -> Option<Facts> {
    self
      .facts
      .read()
      .unwrap_or_else(PoisonError::into_inner)
      .get(username)
      .cloned()
  }

  /// Changes what is known of `username` with `change`, making their entry when `create` or
  /// when it exists; returns whether there was one to change.
  fn update(&self, username: &str, create: bool, change: impl FnOnce(&mut Facts)) -> bool {
    // every change is made whole before the lock is let go
    let mut facts = self.facts.write().unwrap_or_else(PoisonError::into_inner);
    match facts.get_mut(username) {
      Some(known) => change(known),
      None if create => change(facts.entry(username.to_string()).or_default()),
      None => return false,
    }
    true
  }

  fn watched(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
    // the set is changed in single inserts and removals
    self.watched.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

/// What the server said a user shares.
fn shared(stats: &UserStats) -> Shared {
  Shared {
    files: stats.file_count,
    directories: stats.directory_count,
  }
}

impl Directory {
  /// The upload group `username` is in by what is known now, without asking anyone. A user
  /// nothing is known of is in the configuration's group for them, else the default group.
  pub fn group(&self, username: &str) -> String {
    let facts = self.groups.facts(username).unwrap_or_default();
    groups::resolve(&self.config.current().values, username, &facts).name
  }

  /// The upload group `username` is in, after asking the server for their statistics and
  /// their status when nothing is known of them, or always with `force`. A question that fails
  /// is logged and what is known stands, so a user who could not be asked about is in the
  /// default group.
  pub async fn fetch_group(&self, client: &Client, username: &str, force: bool) -> String {
    let known = self.groups.facts(username).is_some();
    if !known || force {
      self.groups.update(username, true, |_| {});
      match client.get_user_statistics(username).await {
        Ok(statistics) => {
          self.groups.update(username, true, |facts| {
            facts.shared = Some(shared(&statistics.stats));
          });
        }
        Err(e) => warn!(username, error = %e, "statistics for the upload group not fetched"),
      }
      match client.get_user_status(username).await {
        Ok(status) => {
          self.groups.update(username, true, |facts| {
            facts.privileged = status.is_privileged;
          });
        }
        Err(e) => warn!(username, error = %e, "status for the upload group not fetched"),
      }
    }
    let group = self.group(username);
    trace!(username, group, "upload group");
    group
  }

  /// Asks the server to tell us when `username`'s status changes, and takes their statistics
  /// and status now. A user the server watches already in this session is left as is; a
  /// failure is logged and changes nothing else.
  pub async fn watch(&self, client: &Client, username: &str) {
    if self.groups.watched().contains(username) {
      return;
    }
    let watched = match client.watch_user(username).await {
      Ok(watched) => watched,
      Err(e) => return warn!(username, error = %e, "user not watched"),
    };
    self.groups.update(username, true, |facts| {
      facts.shared = Some(shared(&watched.stats));
    });
    match client.get_user_status(username).await {
      Ok(status) => {
        self.groups.update(username, true, |facts| {
          facts.privileged = status.is_privileged;
        });
      }
      Err(e) => warn!(username, error = %e, "status of a watched user not fetched"),
    }
    self.groups.watched().insert(username.to_string());
    debug!(username, "user watched");
  }

  /// Every user on the server's list of privileged users is privileged from now on.
  pub fn privileged_listed(&self, usernames: &[String]) {
    for username in usernames {
      self
        .groups
        .update(username, true, |facts| facts.privileged = true);
    }
    debug!(users = usernames.len(), "privileged users listed");
  }

  /// A status the server sent of `username` on its own: their privilege, when we know them.
  pub fn status_received(&self, username: &str, is_privileged: bool) {
    self.groups.update(username, false, |facts| {
      facts.privileged = is_privileged;
    });
  }

  /// Statistics the server sent of `username` on its own: what they share, when we know them.
  pub fn statistics_received(&self, username: &str, stats: &UserStats) {
    self.groups.update(username, false, |facts| {
      facts.shared = Some(shared(stats));
    });
  }

  /// The server session ended, and with it the server's watching of every user.
  pub fn session_ended(&self) {
    self.groups.watched().clear();
  }

  /// `username` is in the user-defined group `group` we put them in from now on, or in none.
  pub(super) fn group_changed(&self, username: &str, group: Option<String>) {
    let create = group.is_some();
    self.groups.update(username, create, |facts| {
      facts.group_override = group;
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn only_users_known_take_what_the_server_says_on_its_own() {
    let cache = GroupCache::new([("alice".to_string(), "friends".to_string())]);
    assert_eq!(
      cache.facts("alice").unwrap().group_override.as_deref(),
      Some("friends")
    );
    assert!(!cache.update("bob", false, |facts| facts.privileged = true));
    assert!(cache.facts("bob").is_none());
    assert!(cache.update("alice", false, |facts| facts.privileged = true));
    assert!(cache.facts("alice").unwrap().privileged);
  }
}
