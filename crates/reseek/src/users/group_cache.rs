//! What the server last said of each user, kept in memory so each user's groups are known at
//! once wherever they are asked: whether the server calls them privileged, their presence and
//! their statistics with when they came, and the groups we put them in. A user enters on a
//! fetch, a watch or the server's list of privileged users, and every user we put in a group is
//! loaded at startup; nobody leaves. What the server pushes on its own is taken only for users
//! known already. Whatever it says that moves the groups a user with a row is shown in pushes
//! their summary.
//!
//! The server pushes a watched user's presence, never their statistics, so those are asked for
//! again when the presence changes on its own, when a user's page is opened or a transfer with
//! them starts, and when a group is worked out from statistics older than four hours. Watching a
//! user takes one turn per user, so a burst of uploads from one user watches them once. The end
//! of a server session forgets which users the server was watching for us, not what it said.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard};
use std::time::{Duration, Instant};

use api_types::state::UserStatistics;
use api_types::users::{CachedUser, StatisticsSnapshot, UserPresence, UserStatus};
use chrono::Utc;
use reseek_shares::lock;
use soulseek_client::{Client, ClientError, LogLimiter};
use soulseek_proto::server::UserStats;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, trace, warn};

use super::groups::{self, Facts, Placement};
use super::store::Stored;
use super::{Directory, Touch};
use crate::config::Options;
use crate::turns::Turns;

/// How old a user's statistics may be when their group is worked out before they are asked
/// for again.
const STALE: Duration = Duration::from_secs(4 * 3600);

static REFETCH_FAILED: LogLimiter = LogLimiter::new();

/// What the server last said of each user known, with the groups we put them in, by user, and
/// the users it watches for us. It is shared with the database writes that make a user's row,
/// which start the row with it.
#[derive(Clone, Default)]
pub struct Reported {
  facts: Arc<RwLock<HashMap<String, Facts>>>,
  /// the users the server watches for us in this session, whose presence it keeps current
  watched: Arc<Mutex<HashSet<String>>>,
}

impl Reported {
  /// What is known of `username`, when anything is.
  pub fn of(&self, username: &str) -> Option<Facts> {
    self.read().get(username).cloned()
  }

  /// What a row made now for `username` starts with: their statistics, which carry the time
  /// they were given, and their presence and privileges only while the server watches them,
  /// as only then does it keep them current. Decided with the watched users held.
  pub fn for_new_row(&self, username: &str) -> Option<Facts> {
    let watched = lock(&self.watched);
    let mut facts = self.of(username)?;
    if !watched.contains(username) {
      facts.presence = None;
    }
    Some(facts)
  }

  /// Changes what is known of `username` with `change`, making their entry when `create` or
  /// when it exists; returns whether there was one to change.
  pub fn update(&self, username: &str, create: bool, change: impl FnOnce(&mut Facts)) -> bool {
    // every change is made whole before the lock is let go
    let mut facts = self.facts.write().unwrap_or_else(PoisonError::into_inner);
    match facts.get_mut(username) {
      Some(known) => change(known),
      None if create => change(facts.entry(username.to_string()).or_default()),
      None => return false,
    }
    true
  }

  fn read(&self) -> RwLockReadGuard<'_, HashMap<String, Facts>> {
    self.facts.read().unwrap_or_else(PoisonError::into_inner)
  }
}

/// What the server last said of every user known, and the users the server watches for us.
#[derive(Default)]
pub struct GroupCache {
  /// what the server said, and the users it watches for us
  facts: Reported,
  /// when each user forgotten by hand in this run was forgotten, so a watch asked for before
  /// does not watch them; taken with `watched` held
  forgotten: Mutex<HashMap<String, Instant>>,
  /// one watch of a user at a time
  watching: Turns,
  /// how many watches of each user completed, so a caller that waited for another's knows
  watches: Mutex<HashMap<String, u64>>,
  /// how many replies to our own status requests are still to come, by user
  asked: Arc<Mutex<HashMap<String, u32>>>,
  /// the users whose statistics are being asked for again
  refetching: Mutex<HashSet<String>>,
  /// the presence of each buddy as we knew it when a server session ended, until a status of
  /// theirs arrives in the next
  missed: Mutex<HashMap<String, UserPresence>>,
  /// held while the groups we put users in change, from the rows to the memberships here, so of
  /// two changes made at once the rows and the memberships keep the same one
  regrouping: tokio::sync::Mutex<()>,
}

impl GroupCache {
  /// The cache knowing the groups `memberships` puts users in, each as the user and the group.
  pub fn new(memberships: impl IntoIterator<Item = (String, String)>) -> Self {
    let mut facts: HashMap<String, Facts> = HashMap::new();
    for (username, group) in memberships {
      facts.entry(username).or_default().memberships.push(group);
    }
    Self {
      facts: Reported {
        facts: Arc::new(RwLock::new(facts)),
        ..Reported::default()
      },
      ..Self::default()
    }
  }

  /// What is known of `username`, when anything is.
  pub fn facts(&self, username: &str) -> Option<Facts> {
    self.facts.of(username)
  }

  /// What is known of every user, for the writes that make a row.
  pub fn reported(&self) -> Reported {
    self.facts.clone()
  }

  /// Changes what is known of `username` with `change`, making their entry when `create` or
  /// when it exists; returns whether there was one to change.
  fn update(&self, username: &str, create: bool, change: impl FnOnce(&mut Facts)) -> bool {
    self.facts.update(username, create, change)
  }

  fn watched(&self) -> MutexGuard<'_, HashSet<String>> {
    lock(&self.facts.watched)
  }

  /// Waits until no other change to the groups we put users in runs, and holds off the next one
  /// until it is dropped.
  pub async fn regrouping(&self) -> tokio::sync::MutexGuard<'_, ()> {
    self.regrouping.lock().await
  }
}

/// What [`Directory::forget_watched`] took away, for [`Directory::unforget_watched`] to put back.
#[derive(Clone, Copy, Debug)]
pub struct Unwatched {
  /// the user was watched
  pub was: bool,
  /// the mark an earlier forget had left
  mark: Option<Instant>,
}

/// What a status the server sent tells beside itself.
pub struct StatusReceived {
  /// it answered our own request
  pub asked: bool,
  /// the user's presence before it, when it was known in this server session
  pub previous: Option<UserPresence>,
}

/// One status request of ours, counted in `asked` from the start. One that ends without its
/// reply, failed or given up by its caller, gives its count back: no reply of its own comes to
/// count it off, and the next status the server pushes would be taken for one.
struct Asking {
  asked: Arc<Mutex<HashMap<String, u32>>>,
  username: String,
  /// its reply came, and the reply's event counts it off
  answered: bool,
}

impl Asking {
  fn new(asked: &Arc<Mutex<HashMap<String, u32>>>, username: &str) -> Self {
    *lock(asked).entry(username.to_string()).or_default() += 1;
    Self {
      asked: asked.clone(),
      username: username.to_string(),
      answered: false,
    }
  }

  /// The reply came; its event counts the request off.
  fn answered(mut self) {
    self.answered = true;
  }
}

impl Drop for Asking {
  fn drop(&mut self) {
    if self.answered {
      return;
    }
    let mut asked = lock(&self.asked);
    // the end of the session clears every count
    let Some(count) = asked.get_mut(&self.username) else {
      return;
    };
    *count -= 1;
    if *count == 0 {
      asked.remove(&self.username);
    }
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
  /// The groups `username`, whose row is `stored` when they have one, is shown in: worked out
  /// from what the server said of them in this run, as the upload engine and the shares have
  /// it, else from what their row kept from before, the last we knew; the engine itself knows
  /// nothing of such a user until it asks the server. The groups we put them in are their row's,
  /// which a change writes first.
  pub(super) fn placement_of(
    &self,
    options: &Options,
    username: &str,
    stored: Option<&Stored>,
  ) -> Placement {
    let cached = self.groups.facts(username).unwrap_or_default();
    let mut facts = if cached.heard() {
      cached
    } else {
      stored.map(Facts::of).unwrap_or_default()
    };
    if let Some(stored) = stored {
      facts.memberships.clone_from(&stored.groups);
    }
    groups::place(options, &facts, self.buddy(username).is_some())
  }

  /// [`Directory::placement_of`] for a user with a row.
  pub(super) fn placement_shown(&self, options: &Options, stored: &Stored) -> Placement {
    self.placement_of(options, &stored.username, Some(stored))
  }

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
  /// status when it has said neither yet, or always with `force`; asking makes their row. A
  /// question that fails is logged and what is known stands, so a user who could not be asked
  /// about is neither privileged nor a leecher.
  pub async fn fetch_placement(
    self: &Arc<Self>,
    client: &Client,
    username: &str,
    force: bool,
  ) -> Placement {
    self.asked_placement(client, username, force, true).await
  }

  /// The groups `username` is in, to let them in when they are in any of `allowed`. When what
  /// is known puts them in none of those and the server's word could, it is asked first as
  /// [`Directory::fetch_placement`] asks, without making their row: a user refused is no reason
  /// to know them, and the name may be one they picked for the occasion.
  pub async fn placement_letting_in(
    self: &Arc<Self>,
    client: &Client,
    username: &str,
    allowed: &[String],
  ) -> Placement {
    let placement = self.placement(username);
    if allowed.iter().any(|group| placement.is_in(group))
      || !allowed.iter().any(|group| groups::decided_by_server(group))
    {
      return placement;
    }
    self.asked_placement(client, username, false, false).await
  }

  /// [`Directory::fetch_placement`], making the user's row when it asks only with `makes_row`.
  async fn asked_placement(
    self: &Arc<Self>,
    client: &Client,
    username: &str,
    force: bool,
    makes_row: bool,
  ) -> Placement {
    let known = self
      .groups
      .facts(username)
      .is_some_and(|facts| facts.statistics.is_some() || facts.presence.is_some());
    if !known || force {
      self.groups.update(username, true, |_| {});
      let statistics = if makes_row {
        self.statistics_of(client, username).await
      } else {
        self.statistics_reported(client, username).await
      };
      if let Err(e) = statistics {
        warn!(username, error = %e, "statistics for the user's groups not fetched");
      }
      if let Err(e) = self.status_of(client, username).await {
        warn!(username, error = %e, "status for the user's groups not fetched");
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

  /// Asks the server for `username`'s statistics now for ourselves, through the API or for
  /// their groups, keeps them and touches the user, which makes their row when there was none
  /// when asked. A user with a row then is only filled in, so forgetting them while the answer
  /// is on its way is not undone.
  pub async fn statistics_of(
    &self,
    client: &Client,
    username: &str,
  ) -> Result<StatisticsSnapshot, ClientError> {
    let known = self.known.has(username);
    let taken = self.statistics_reported(client, username).await?;
    if !known {
      self.touch(username, Touch::Asked).await;
    }
    Ok(taken)
  }

  /// Asks the server for `username`'s statistics again, keeps them and fills them into the
  /// user's row when there is one.
  async fn statistics_reported(
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
    Ok(self.statistics_kept(username, &fetched.stats).await)
  }

  /// Statistics the server gave: kept, and filled into the user's row when there is one.
  async fn statistics_fetched(&self, username: &str, stats: &UserStats) -> StatisticsSnapshot {
    let taken = self.statistics_kept(username, stats).await;
    let touch = Touch::Statistics(taken.statistics.clone());
    self.touch(username, touch).await;
    taken
  }

  /// Statistics we asked for, kept for the upload groups.
  async fn statistics_kept(&self, username: &str, stats: &UserStats) -> StatisticsSnapshot {
    let taken = snapshot(stats);
    self
      .learned(username, true, |facts| {
        facts.statistics = Some(taken.clone());
      })
      .await;
    taken
  }

  /// Changes what the server said of `username` with `change`, making their entry when `create`
  /// or when it exists, and pushes the summary of a user with a row whose groups it moves.
  async fn learned(&self, username: &str, create: bool, change: impl FnOnce(&mut Facts)) {
    let options = self.config.current();
    let mut moved = false;
    self.groups.update(username, create, |facts| {
      let before = facts.clone();
      change(facts);
      moved = groups::moves(&options.values, &before, facts);
    });
    if moved && self.known.has(username) {
      self.forget(username);
      self.push(username).await;
    }
  }

  /// Asks the server for `username`'s status now and keeps it. The reply is also raised as a
  /// status change, which [`Directory::status_received`] then knows we asked for.
  pub async fn status_of(
    &self,
    client: &Client,
    username: &str,
  ) -> Result<UserStatus, ClientError> {
    let asking = Asking::new(&self.groups.asked, username);
    // the request runs on a task of its own, so a caller that gives up while its reply is in
    // hand, not yet taken, still leaves the count for the reply's event
    let given_up = CancellationToken::new();
    let _gives_up = given_up.clone().drop_guard();
    let (client, name) = (client.clone(), username.to_string());
    let asked = self.tasks.spawn(
      async move {
        let status = tokio::select! {
          biased;
          status = client.get_user_status(&name) => status,
          () = given_up.cancelled() => Err(ClientError::Canceled),
        };
        if status.is_ok() {
          asking.answered();
        }
        status
      }
      .in_current_span(),
    );
    let status = match asked.await {
      Ok(status) => status?,
      Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
      Err(e) => return Err(ClientError::client(e.to_string(), None)),
    };
    let presence = UserPresence::from(status.presence);
    self
      .status_fetched(username, presence, status.is_privileged)
      .await;
    Ok(UserStatus {
      presence,
      is_privileged: status.is_privileged,
    })
  }

  /// The status we asked for of `username`, kept. The same reply is also raised as a status
  /// change, and either may be taken first, so both tell of a change missed while disconnected.
  pub(super) async fn status_fetched(
    &self,
    username: &str,
    presence: UserPresence,
    is_privileged: bool,
  ) {
    self.tell_missed(username, presence);
    self
      .learned(username, true, |facts| {
        facts.presence = Some(presence);
        facts.privileged = is_privileged;
      })
      .await;
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
    let asked = Instant::now();
    let watched = client.watch_user(username).await?;
    // the server watches them from its answer on, so the status asked for next, which the
    // follower of the client's events may take before this task does, finds them watched
    if !self.watch_taken(username, asked) {
      debug!(
        username,
        "user forgotten while the watch was asked for; not watched"
      );
      self.unwatch_forgotten(username).await;
      return Ok(());
    }
    self.statistics_fetched(username, &watched.stats).await;
    if let Err(e) = self.status_of(client, username).await {
      warn!(username, error = %e, "status of a watched user not fetched");
    }
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

  /// Puts `username`, whose watch was asked for at `asked`, among the watched users, unless
  /// they were forgotten since; says whether they were. Decided with the watched users held,
  /// as [`Directory::forget_watched`] forgets with them held, so the two never cross.
  fn watch_taken(&self, username: &str, asked: Instant) -> bool {
    let mut watched = self.groups.watched();
    let forgotten = lock(&self.groups.forgotten).get(username).copied();
    if forgotten.is_some_and(|forgotten| asked <= forgotten) {
      return false;
    }
    watched.insert(username.to_string());
    true
  }

  /// Forgets `username` for the watched users: off them, without telling the server, and
  /// marked forgotten now, both with the watched users held. A status of theirs handled from
  /// here on makes no touch, and a watch asked for before does not watch them. Says what
  /// [`Directory::unforget_watched`] puts back.
  pub(super) fn forget_watched(&self, username: &str) -> Unwatched {
    let mut watched = self.groups.watched();
    let was = watched.remove(username);
    let mark = lock(&self.groups.forgotten).insert(username.to_string(), Instant::now());
    Unwatched { was, mark }
  }

  /// Undoes [`Directory::forget_watched`] when there was nobody to forget: `username` is
  /// watched again when they were, and an earlier forget's mark is kept as it was.
  pub(super) fn unforget_watched(&self, username: &str, before: Unwatched) {
    let mut watched = self.groups.watched();
    if before.was {
      watched.insert(username.to_string());
    }
    let mut forgotten = lock(&self.groups.forgotten);
    match before.mark {
      Some(mark) => forgotten.insert(username.to_string(), mark),
      None => forgotten.remove(username),
    };
  }

  /// Tells the server to stop watching `username`, who was forgotten and is off the watched
  /// users already.
  pub(super) async fn unwatch_forgotten(&self, username: &str) {
    let Some(client) = self.client.get() else {
      return;
    };
    match client.unwatch_user(username).await {
      Ok(()) => debug!(username, "forgotten user no longer watched"),
      Err(e) => debug!(
        username,
        error = %e,
        "the server was not told to stop watching a forgotten user; what it says is dropped"
      ),
    }
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
        if let Err(e) = this.statistics_reported(&client, &username).await
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
  pub async fn privileged_listed(&self, usernames: &[String]) {
    for username in usernames {
      self
        .learned(username, true, |facts| facts.privileged = true)
        .await;
    }
    debug!(users = usernames.len(), "privileged users listed");
  }

  /// A status the server sent of `username`: kept when we know them, and counted off when it
  /// answered our own request. Says whether we asked for it and the presence known before in
  /// this session, or none when this is the first word on a buddy after a reconnect, whose
  /// change while we were not connected is told of here unless our own request's reply was
  /// taken first.
  pub async fn status_received(
    &self,
    username: &str,
    presence: UserPresence,
    is_privileged: bool,
  ) -> StatusReceived {
    let first = self.tell_missed(username, presence);
    let asked = {
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
    };
    let mut previous = None;
    self
      .learned(username, false, |facts| {
        previous = facts.presence.replace(presence);
        facts.privileged = is_privileged;
      })
      .await;
    if first {
      previous = None;
    }
    StatusReceived { asked, previous }
  }

  /// Statistics the server sent of `username` on its own: kept when we know them.
  pub async fn statistics_received(&self, username: &str, stats: &UserStats) {
    self
      .learned(username, false, |facts| {
        facts.statistics = Some(snapshot(stats));
      })
      .await;
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
    let facts = self.groups.facts.read();
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_answered_status_request_stays_counted_for_its_reply_and_one_given_up_does_not() {
    let asked = Arc::new(Mutex::new(HashMap::new()));
    Asking::new(&asked, "alice").answered();
    assert_eq!(
      lock(&asked).get("alice"),
      Some(&1),
      "the reply's event counts it off"
    );
    drop(Asking::new(&asked, "alice"));
    assert_eq!(
      lock(&asked).get("alice"),
      Some(&1),
      "only the answered one is left"
    );
    lock(&asked).clear();
    drop(Asking::new(&asked, "alice"));
    assert!(lock(&asked).is_empty());
  }
}
