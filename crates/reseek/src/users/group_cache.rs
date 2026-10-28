//! What the server last said of each user, kept in memory so each user's upload group is known
//! at once wherever it is asked: whether the server calls them privileged, their presence and
//! their statistics with when they came, and the group we put them in. A user enters on a
//! fetch, a watch or the server's list of privileged users, and every user we put in a group is
//! loaded at startup; nobody leaves. What the server pushes on its own is taken only for users
//! known already.
//!
//! The server pushes a watched user's presence, never their statistics, so those are asked for
//! again when the presence changes on its own, when a user's page is opened or a transfer with
//! them starts, and when a group is worked out from statistics older than four hours. Watching a
//! user takes one turn per user, so a burst of uploads from one user watches them once. The end
//! of a server session forgets which users the server was watching for us, not what it said.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

use api_types::state::UserStatistics;
use api_types::users::{CachedUser, StatisticsSnapshot, UserPresence, UserStatus};
use chrono::Utc;
use soulseek_client::{Client, ClientError, LogLimiter};
use soulseek_proto::server::UserStats;
use tracing::{Instrument, debug, info, trace, warn};

use super::groups::{self, Facts};
use super::{Directory, Touch};
use crate::sync::lock;
use crate::turns::Turns;

/// How old a user's statistics may be when their group is worked out before they are asked
/// for again.
const STALE: Duration = Duration::from_secs(4 * 3600);

static REFETCH_FAILED: LogLimiter = LogLimiter::new();

/// What the server last said of every user known, and the users the server watches for us.
#[derive(Default)]
pub struct GroupCache {
  facts: RwLock<HashMap<String, Facts>>,
  watched: Mutex<HashSet<String>>,
  /// one watch of a user at a time
  watching: Turns,
  /// how many watches of each user completed, so a caller that waited for another's knows
  watches: Mutex<HashMap<String, u64>>,
  /// how many replies to our own status requests are still to come, by user
  asked: Mutex<HashMap<String, u32>>,
  /// the users whose statistics are being asked for again
  refetching: Mutex<HashSet<String>>,
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
      ..Self::default()
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

  fn watched(&self) -> MutexGuard<'_, HashSet<String>> {
    lock(&self.watched)
  }
}

/// Statistics as the server said them, now.
fn snapshot(stats: &UserStats) -> StatisticsSnapshot {
  StatisticsSnapshot {
    statistics: statistics(stats),
    updated_at: Utc::now(),
  }
}

fn statistics(stats: &UserStats) -> UserStatistics {
  UserStatistics {
    average_speed: stats.average_speed,
    directory_count: stats.directory_count,
    file_count: stats.file_count,
    upload_count: stats.upload_count.into(),
  }
}

impl Directory {
  /// The upload group `username` is in by what is known now, without waiting for anyone. A user
  /// nothing is known of is in the configuration's group for them, else the default group;
  /// statistics older than four hours are asked for again meanwhile.
  pub fn group(self: &Arc<Self>, username: &str) -> String {
    let facts = self.groups.facts(username).unwrap_or_default();
    if facts.statistics.as_ref().is_some_and(|s| {
      (Utc::now() - s.updated_at)
        .to_std()
        .is_ok_and(|age| age > STALE)
    }) {
      self.refetch_statistics(username);
    }
    groups::resolve(&self.config.current().values, username, &facts).name
  }

  /// The upload group `username` is in, after asking the server for their statistics and
  /// their status when nothing is known of them, or always with `force`. A question that fails
  /// is logged and what is known stands, so a user who could not be asked about is in the
  /// default group.
  pub async fn fetch_group(
    self: &Arc<Self>,
    client: &Client,
    username: &str,
    force: bool,
  ) -> String {
    let known = self.groups.facts(username).is_some();
    if !known || force {
      self.groups.update(username, true, |_| {});
      if let Err(e) = self.statistics_of(client, username).await {
        warn!(username, error = %e, "statistics for the upload group not fetched");
      }
      if let Err(e) = self.status_of(client, username).await {
        warn!(username, error = %e, "status for the upload group not fetched");
      }
    }
    let group = self.group(username);
    trace!(username, group, "upload group");
    group
  }

  /// Asks the server for `username`'s statistics now, keeps them and touches the user.
  pub async fn statistics_of(
    &self,
    client: &Client,
    username: &str,
  ) -> Result<StatisticsSnapshot, ClientError> {
    let fetched = client.get_user_statistics(username).await?;
    Ok(self.statistics_fetched(username, &fetched.stats).await)
  }

  /// Statistics we asked for: kept, and the user touched with them.
  async fn statistics_fetched(&self, username: &str, stats: &UserStats) -> StatisticsSnapshot {
    let taken = snapshot(stats);
    self.groups.update(username, true, |facts| {
      facts.statistics = Some(taken.clone());
    });
    self
      .touch(username, Touch::Statistics(taken.statistics.clone()))
      .await;
    taken
  }

  /// Asks the server for `username`'s status now and keeps it. The reply is also raised as a
  /// status change, which [`Directory::status_received`] then knows we asked for.
  pub async fn status_of(
    &self,
    client: &Client,
    username: &str,
  ) -> Result<UserStatus, ClientError> {
    *lock(&self.groups.asked)
      .entry(username.to_string())
      .or_default() += 1;
    let fetched = client.get_user_status(username).await;
    let status = match fetched {
      Ok(status) => status,
      Err(e) => {
        // no reply comes that would be counted off
        let mut asked = lock(&self.groups.asked);
        if let Some(count) = asked.get_mut(username) {
          *count = count.saturating_sub(1);
          if *count == 0 {
            asked.remove(username);
          }
        }
        return Err(e);
      }
    };
    let presence = UserPresence::from(status.presence);
    self.groups.update(username, true, |facts| {
      facts.presence = Some(presence);
      facts.privileged = status.is_privileged;
    });
    Ok(UserStatus {
      presence,
      is_privileged: status.is_privileged,
    })
  }

  /// Asks the server to tell us when `username`'s status changes, and takes their statistics
  /// and status now. A user the server watches already in this session is left as is; a
  /// failure is logged and changes nothing else.
  pub async fn watch(&self, client: &Client, username: &str) {
    if self.is_watched(username) {
      return;
    }
    if let Err(e) = self.watch_once(client, username, false).await {
      warn!(username, error = %e, "user not watched");
    }
  }

  /// Watches `username` as [`Directory::watch`] does, asking the server again even when it
  /// watches them already, and says how it went.
  pub async fn watch_again(&self, client: &Client, username: &str) -> Result<(), ClientError> {
    self.watch_once(client, username, true).await
  }

  /// The watch itself, one per user at a time: a caller that waited while another watched the
  /// user takes that watch for its own, and one that finds the user watched already is done
  /// unless `again`.
  async fn watch_once(
    &self,
    client: &Client,
    username: &str,
    again: bool,
  ) -> Result<(), ClientError> {
    let completed = |groups: &GroupCache| lock(&groups.watches).get(username).copied();
    let before = completed(&self.groups);
    let _turn = self.groups.watching.take(username).await;
    if completed(&self.groups) != before || (!again && self.is_watched(username)) {
      return Ok(());
    }
    let watched = client.watch_user(username).await?;
    self.statistics_fetched(username, &watched.stats).await;
    if let Err(e) = self.status_of(client, username).await {
      warn!(username, error = %e, "status of a watched user not fetched");
    }
    self.groups.watched().insert(username.to_string());
    *lock(&self.groups.watches)
      .entry(username.to_string())
      .or_default() += 1;
    info!(username, "Added user to watch list");
    Ok(())
  }

  /// Stops the server's status reports of `username`.
  pub async fn unwatch(&self, client: &Client, username: &str) -> Result<(), ClientError> {
    client.unwatch_user(username).await?;
    self.groups.watched().remove(username);
    debug!(username, "user no longer watched");
    Ok(())
  }

  /// Asks the server for the watched `username`'s statistics again in the background, unless
  /// that is under way; the server pushes their presence and not what they share. The stop
  /// waits for it.
  pub fn refetch_statistics(self: &Arc<Self>, username: &str) {
    let Some(client) = self.client.get().cloned() else {
      return;
    };
    if !lock(&self.groups.refetching).insert(username.to_string()) {
      return;
    }
    let (this, username) = (self.clone(), username.to_string());
    self.tasks.spawn(
      async move {
        if let Err(e) = this.statistics_of(&client, &username).await
          && let Some(suppressed) = REFETCH_FAILED.allow()
        {
          warn!(username, suppressed, error = %e, "statistics of a user not fetched again");
        }
        lock(&this.groups.refetching).remove(&username);
      }
      .in_current_span(),
    );
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

  /// A status the server sent of `username`: kept when we know them, and counted off when it
  /// answered our own request. Returns whether we asked for it.
  pub fn status_received(
    &self,
    username: &str,
    presence: UserPresence,
    is_privileged: bool,
  ) -> bool {
    self.groups.update(username, false, |facts| {
      facts.presence = Some(presence);
      facts.privileged = is_privileged;
    });
    let mut asked = lock(&self.groups.asked);
    match asked.get_mut(username) {
      Some(count) => {
        *count -= 1;
        if *count == 0 {
          asked.remove(username);
        }
        true
      }
      None => false,
    }
  }

  /// Statistics the server sent of `username` on its own: kept when we know them.
  pub fn statistics_received(&self, username: &str, stats: &UserStats) {
    self.groups.update(username, false, |facts| {
      facts.statistics = Some(snapshot(stats));
    });
  }

  /// Whether the server watches `username` for us in this session.
  pub fn is_watched(&self, username: &str) -> bool {
    self.groups.watched().contains(username)
  }

  /// The users the server watches for us, by name.
  pub fn watched_users(&self) -> Vec<String> {
    let mut users: Vec<String> = self.groups.watched().iter().cloned().collect();
    users.sort();
    users
  }

  /// Every user the cache knows, by name, with what the server last said of them.
  pub fn cached_users(&self) -> Vec<CachedUser> {
    let options = self.config.current().values.clone();
    let facts = self
      .groups
      .facts
      .read()
      .unwrap_or_else(PoisonError::into_inner);
    let mut users: Vec<CachedUser> = facts
      .iter()
      .map(|(username, facts)| CachedUser {
        username: username.clone(),
        group: groups::configured(&options, username).map(|(name, _)| name),
        statistics: facts.statistics.clone(),
        status: facts.presence.map(|presence| UserStatus {
          presence,
          is_privileged: facts.privileged,
        }),
      })
      .collect();
    users.sort_by(|a, b| a.username.cmp(&b.username));
    users
  }

  /// The server session ended, and with it the server's watching of every user.
  pub fn session_ended(&self) {
    self.groups.watched().clear();
    lock(&self.groups.asked).clear();
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
