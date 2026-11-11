//! What the server last said of each user, kept in memory so each user's groups are known at
//! once wherever they are asked: whether the server calls them privileged, their presence and
//! their statistics with when they came, and the groups we put them in. A user enters on a
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
use reseek_shares::lock;
use soulseek_client::{Client, ClientError, LogLimiter};
use soulseek_proto::server::UserStats;
use tracing::{Instrument, debug, info, trace, warn};

use super::groups::{self, Facts, Placement};
use super::{Directory, Touch};
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
  /// the presence of each buddy as we knew it when a server session ended, until a status of
  /// theirs arrives in the next
  missed: Mutex<HashMap<String, UserPresence>>,
}

impl GroupCache {
  /// The cache knowing the groups `memberships` puts users in, each as the user and the group.
  pub fn new(memberships: impl IntoIterator<Item = (String, String)>) -> Self {
    let mut facts: HashMap<String, Facts> = HashMap::new();
    for (username, group) in memberships {
      facts.entry(username).or_default().memberships.push(group);
    }
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

/// What a status the server sent tells beside itself.
pub struct StatusReceived {
  /// it answered our own request
  pub asked: bool,
  /// the user's presence before it, when it was known in this server session
  pub previous: Option<UserPresence>,
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
  /// The groups `username` is in by what is known now, without waiting for anyone; statistics
  /// older than four hours are asked for again meanwhile.
  pub fn placement(self: &Arc<Self>, username: &str) -> Placement {
    let facts = self.groups.facts(username).unwrap_or_default();
    if facts.statistics.as_ref().is_some_and(|s| {
      (Utc::now() - s.updated_at)
        .to_std()
        .is_ok_and(|age| age > STALE)
    }) {
      self.refetch_statistics(username);
    }
    let buddy = self.buddy(username).is_some();
    groups::place(&self.config.current().values, &facts, buddy)
  }

  /// The group whose queue `username` waits in, by what is known now, without waiting for
  /// anyone.
  pub fn queue_group(self: &Arc<Self>, username: &str) -> String {
    self.placement(username).queue
  }

  /// The group whose speed limit `username`'s uploads count against, by what is known now,
  /// without waiting for anyone.
  pub fn speed_group(self: &Arc<Self>, username: &str) -> String {
    self.placement(username).speed
  }

  /// The groups `username` is in, after asking the server for their statistics and their
  /// status when it has said neither yet, or always with `force`. A question that fails is
  /// logged and what is known stands, so a user who could not be asked about is neither
  /// privileged nor a leecher.
  pub async fn fetch_placement(
    self: &Arc<Self>,
    client: &Client,
    username: &str,
    force: bool,
  ) -> Placement {
    let known = self
      .groups
      .facts(username)
      .is_some_and(|facts| facts.statistics.is_some() || facts.presence.is_some());
    if !known || force {
      self.groups.update(username, true, |_| {});
      if let Err(e) = self.statistics_of(client, username).await {
        warn!(username, error = %e, "statistics for the upload groups not fetched");
      }
      if let Err(e) = self.status_of(client, username).await {
        warn!(username, error = %e, "status for the upload groups not fetched");
      }
    }
    let placement = self.placement(username);
    trace!(
      username,
      queue = placement.queue,
      speed = placement.speed,
      "upload groups"
    );
    placement
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

  /// Asks the server for `username`'s statistics now and keeps them for the upload groups,
  /// leaving the user's row to the caller.
  pub(super) async fn statistics_untouched(
    &self,
    client: &Client,
    username: &str,
  ) -> Result<StatisticsSnapshot, ClientError> {
    let fetched = client.get_user_statistics(username).await?;
    Ok(self.statistics_kept(username, &fetched.stats))
  }

  /// Statistics we asked for: kept, and the user touched with them.
  async fn statistics_fetched(&self, username: &str, stats: &UserStats) -> StatisticsSnapshot {
    let taken = self.statistics_kept(username, stats);
    self
      .touch(username, Touch::Statistics(taken.statistics.clone()))
      .await;
    taken
  }

  /// Statistics we asked for, kept for the upload groups.
  fn statistics_kept(&self, username: &str, stats: &UserStats) -> StatisticsSnapshot {
    let taken = snapshot(stats);
    self.groups.update(username, true, |facts| {
      facts.statistics = Some(taken.clone());
    });
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
    self.status_fetched(username, presence, status.is_privileged);
    Ok(UserStatus {
      presence,
      is_privileged: status.is_privileged,
    })
  }

  /// The status we asked for of `username`, kept. The same reply is also raised as a status
  /// change, and either may be taken first, so both tell of a change missed while disconnected.
  pub(super) fn status_fetched(&self, username: &str, presence: UserPresence, is_privileged: bool) {
    self.tell_missed(username, presence);
    self.groups.update(username, true, |facts| {
      facts.presence = Some(presence);
      facts.privileged = is_privileged;
    });
  }

  /// Tells of a buddy's move from the presence we knew when the last server session ended to
  /// `presence`, the first we hear in this one; says whether it was that first word. Only the
  /// first caller finds the presence we knew, so a change is told once.
  fn tell_missed(&self, username: &str, presence: UserPresence) -> bool {
    let Some(before) = lock(&self.groups.missed).remove(username) else {
      return false;
    };
    if before != presence {
      self.presence_moved(username, before, presence);
    }
    true
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
  /// answered our own request. Says whether we asked for it and the presence known before in
  /// this session, or none when this is the first word on a buddy after a reconnect, whose
  /// change while we were not connected is told of here unless our own request's reply was
  /// taken first.
  pub fn status_received(
    &self,
    username: &str,
    presence: UserPresence,
    is_privileged: bool,
  ) -> StatusReceived {
    let first = self.tell_missed(username, presence);
    let mut previous = None;
    self.groups.update(username, false, |facts| {
      previous = facts.presence.replace(presence);
      facts.privileged = is_privileged;
    });
    if first {
      previous = None;
    }
    let mut asked = lock(&self.groups.asked);
    let asked = match asked.get_mut(username) {
      Some(count) => {
        *count -= 1;
        if *count == 0 {
          asked.remove(username);
        }
        true
      }
      None => false,
    };
    StatusReceived { asked, previous }
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
    let facts = self
      .groups
      .facts
      .read()
      .unwrap_or_else(PoisonError::into_inner);
    let mut users: Vec<CachedUser> = facts
      .iter()
      .map(|(username, facts)| CachedUser {
        username: username.clone(),
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

  /// The server session ended, and with it the server's watching of every user. The presence
  /// of each buddy is kept as what we last knew, against which the next session's first word
  /// on them is told of.
  pub fn session_ended(&self) {
    self.groups.watched().clear();
    lock(&self.groups.asked).clear();
    let known: HashMap<String, UserPresence> = self
      .buddy_names()
      .into_iter()
      .filter_map(|username| {
        let presence = self.groups.facts(&username)?.presence?;
        Some((username, presence))
      })
      .collect();
    *lock(&self.groups.missed) = known;
  }

  /// Makes `change` to the groups or buddy standing of `usernames`, then has the shares forget
  /// their browse views when it changed which shares any of them sees, so the view they had is
  /// not rewritten with the shares for no one.
  pub(super) fn regroup<'a>(
    &self,
    usernames: impl IntoIterator<Item = &'a str> + Clone,
    change: impl FnOnce(),
  ) {
    let before: Vec<_> = usernames
      .clone()
      .into_iter()
      .map(|u| self.sees(u))
      .collect();
    change();
    let after = usernames.into_iter().map(|u| self.sees(u));
    if after.ne(before) {
      self.shares.forget_browse_views();
    }
  }

  /// `username` left the group `from`, for the group `to` when given.
  pub(super) fn membership_moved(&self, username: &str, from: &str, to: Option<&str>) {
    self.groups.update(username, false, |facts| {
      facts.memberships.retain(|group| group != from);
      if let Some(to) = to
        && !facts.memberships.iter().any(|group| group == to)
      {
        facts.memberships.push(to.to_string());
        facts.memberships.sort();
      }
    });
  }

  /// `username` is in the groups `memberships` from now on.
  pub(super) fn memberships_changed(&self, username: &str, memberships: Vec<String>) {
    let create = !memberships.is_empty();
    self.groups.update(username, create, |facts| {
      facts.memberships = memberships;
    });
  }
}
