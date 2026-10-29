//! The directory of users we dealt with: the row kept for each in `users.db`, touched as we
//! deal with them, our notes, tags, pins, mutes and groups for them, the figures of their
//! transfers, and who is blocked from what.
//!
//! [`Directory`] is the engine: the API's users directory and blocking calls, and what the
//! network engines ask of it: [`Directory::touch`], [`Directory::is_blocked`], each user's
//! upload group, [`Directory::group`] and [`Directory::fetch_group`], and watching users,
//! [`Directory::watch`]. [`Users`] answers the API's user calls with it and with what the
//! network says about users.

mod blocking;
mod blocklist;
mod browse;
mod group_cache;
mod groups;
mod known;
mod listing;
mod network;
mod profile;
mod retention;
mod service;
mod store;
#[cfg(test)]
mod tests;
mod touch;
mod transfers;

pub use blocking::reason_fits;
pub use groups::{DEFAULT as DEFAULT_GROUP, LEECHERS, PRIVILEGED};

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use api_types::blocking::{Block, BlockScope};
use api_types::events::{Event, UnblockReason, UserUnblocked};
use api_types::live::LiveFrame;
use api_types::users::{KnownUser, UserEndpoint, UserRemoved, UserSummary};
use cascade::Watcher;
use chrono::Utc;
use rusqlite::params;
use soulseek_client::Client;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, debug, error, info, info_span, warn};

pub use network::Users;
pub(crate) use service::picture_response;
pub use touch::{PeerInfo, Touch};

use crate::config::{Options, RETENTION_SWEEP};
use crate::db::{Database, DbError, Location};
use crate::events::EventBus;
use crate::live::Live;
use crate::sync::lock;
use blocking::Blocks;
use blocklist::Blocklist;
use group_cache::GroupCache;
use known::Known;
use store::Stored;
use touch::Toucher;
use transfers::FigureCache;

/// How long a summary is served again without being built anew.
const SUMMARY_FOR: Duration = Duration::from_secs(60);
/// How often expired blocks are removed.
const EXPIRY_SWEEP: Duration = Duration::from_secs(60);
/// How long the stop waits for statistics being fetched again.
const STOPPING: Duration = Duration::from_secs(5);

/// The users directory and the blocks.
pub struct Directory {
  db: Database,
  transfers: Database,
  /// `messaging.db`, for what our private messages with a user add up to
  messaging: Database,
  /// the Soulseek client, once the network is built
  client: OnceLock<Client>,
  /// the statistics fetched again in the background, so the stop waits for them
  tasks: TaskTracker,
  config: Watcher<Options>,
  live: Live,
  events: EventBus,
  toucher: Toucher,
  blocks: Blocks,
  /// what each user's upload group is worked out from
  groups: GroupCache,
  figures: FigureCache,
  summaries: Mutex<HashMap<String, (Instant, UserSummary)>>,
  /// who has a row, with their country
  known: Known,
}

impl Directory {
  /// Opens `users.db` in `location` and loads the blocks, the groups we put users in and the
  /// blocklist file, then starts the touch writer, the expiry and retention sweeps and the
  /// follower of the configuration, which all stop when `shutdown` is canceled. A user's
  /// figures are read from `transfers` and `messaging`.
  pub async fn open(
    location: &Location,
    transfers: Database,
    messaging: Database,
    config: Watcher<Options>,
    live: Live,
    events: EventBus,
    shutdown: CancellationToken,
  ) -> Result<Arc<Self>, String> {
    let db = Database::open(&store::SCHEMA, location)
      .await
      .map_err(|e| format!("users.db: {e}"))?;
    let (ours, overrides, known) = db
      .read(|conn| {
        Ok((
          blocking::stored(conn, Utc::now())?,
          store::overrides(conn)?,
          Known::read(conn)?,
        ))
      })
      .await
      .map_err(|e| format!("users.db: {e}"))?;
    let options = config.current().values.clone();
    let blocks = Blocks::new(&options, &ours);
    if let Some(path) = blocklist_file(&options) {
      let list = load_blocklist(path.clone()).await.map_err(|e| {
        format!(
          "Fatal error loading blacklist from file {}: {e}",
          path.display()
        )
      })?;
      blocks.set_blocklist(Some((path, list)));
    }
    let (toucher, receiver) = Toucher::channel();
    let directory = Arc::new(Self {
      db,
      transfers,
      messaging,
      client: OnceLock::new(),
      tasks: TaskTracker::new(),
      config,
      live,
      events,
      toucher,
      blocks,
      groups: GroupCache::new(overrides),
      figures: FigureCache::default(),
      summaries: Mutex::new(HashMap::new()),
      known,
    });
    tokio::spawn(
      directory
        .clone()
        .write_touches(receiver, shutdown.clone())
        .instrument(info_span!("user_touches")),
    );
    tokio::spawn(
      directory
        .clone()
        .sweep(shutdown.clone())
        .instrument(info_span!("user_sweeps")),
    );
    tokio::spawn(
      directory
        .clone()
        .follow_configuration(shutdown)
        .instrument(info_span!("user_blocks")),
    );
    Ok(directory)
  }

  /// `users.db`.
  pub fn database(&self) -> &Database {
    &self.db
  }

  /// Takes the Soulseek client the network engines use, once it is built.
  pub fn attach_client(&self, client: Client) {
    if self.client.set(client).is_err() {
      warn!("the client was attached to the users directory already");
    }
  }

  /// Waits, at shutdown, for the statistics being fetched again.
  pub async fn stop(&self) {
    self.tasks.close();
    if tokio::time::timeout(STOPPING, self.tasks.wait())
      .await
      .is_err()
    {
      debug!(
        running = self.tasks.len(),
        "statistics still being fetched at shutdown"
      );
    }
  }

  /// Whether the configuration's blacklist names `username`; nothing of ours counts.
  pub fn configuration_blocks(&self, username: &str) -> bool {
    self.blocks.configuration_blocks(username)
  }

  /// Takes the countries room member lists gave, as `(username, code)`; only a user with a row
  /// whose country moved is written, and a user without a row is not made one.
  pub async fn countries_seen(&self, countries: Vec<(String, String)>) {
    let countries = self.known.news(countries);
    if countries.is_empty() {
      return;
    }
    let written = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let mut changed = Vec::new();
        {
          let mut statement = tx.prepare_cached(
            "UPDATE users SET country_code = ?2 WHERE username = ?1 \
             AND country_code IS NOT ?2",
          )?;
          for (username, code) in countries {
            if statement.execute(params![username, code])? > 0 {
              changed.push((username, code));
            }
          }
        }
        tx.commit()?;
        Ok(changed)
      })
      .await;
    match written {
      Ok(changed) => {
        for (username, code) in changed {
          self.known.set_country(&username, code);
          self.changed(&username).await;
        }
      }
      Err(e) => warn!(error = %e, "countries from a room member list not written"),
    }
  }

  /// `username`'s address from the client's cache, never a round trip.
  fn endpoint_of(&self, username: &str) -> Option<UserEndpoint> {
    let endpoint = self.client.get()?.cached_user_endpoint(username)?;
    Some(endpoint.into())
  }

  /// Adds to `user` what is not in `users.db`: their cached address and our conversation with
  /// them.
  async fn with_network_and_conversation(&self, mut user: KnownUser) -> KnownUser {
    user.endpoint = self.endpoint_of(&user.username);
    let name = user.username.clone();
    match self
      .messaging
      .read(move |conn| crate::messaging::conversation_summary(conn, &name))
      .await
    {
      Ok(conversation) => user.conversation = conversation,
      Err(e) => warn!(username = user.username, error = %e, "conversation not read for a user"),
    }
    user
  }

  /// Marks that we dealt with `username` for the reason `touch` gives, storing what came with
  /// it; makes the user's row when there is none, except for a room message. The write
  /// happens within a second. A picture is kept only while `users.cache_pictures` is on and
  /// it is no larger than `users.picture_max_bytes`.
  pub async fn touch(&self, username: &str, touch: Touch) {
    let touch = match touch {
      Touch::Info(mut info) => {
        let users = &self.config.current().values.users;
        let fits = info
          .picture
          .as_ref()
          .is_some_and(|p| p.len() as u64 <= users.picture_max_bytes.as_u64());
        if !(users.cache_pictures && fits) {
          info.picture = None;
        }
        Touch::Info(info)
      }
      other => other,
    };
    // a touch dropped for a full channel is logged where it is dropped
    self.toucher.send(username, touch).await;
  }

  /// Whether `username`, connecting from `ip` when known, is blocked from `scope`, by our
  /// block or the configuration's blacklist.
  pub fn is_blocked(&self, username: &str, ip: Option<IpAddr>, scope: BlockScope) -> bool {
    self.blocks.is_blocked(username, ip, scope)
  }

  /// Whether a block made through the API, not the configuration, blocks `username` from
  /// `scope`.
  pub fn blocked_through_api(&self, username: &str, scope: BlockScope) -> bool {
    self.blocks.ours_blocks(username, scope)
  }

  /// Whether a remembered answer says `username` is blocked from `scope`; a user without one is
  /// taken as not blocked, and nothing is worked out.
  pub fn remembers_blocked(&self, username: &str, scope: BlockScope) -> bool {
    self.blocks.remembers_blocked(username, scope)
  }

  /// The row of `username` and the block that applies to them.
  async fn stored(&self, username: &str) -> Result<(Option<Stored>, Option<Block>), DbError> {
    let name = username.to_string();
    let now = Utc::now();
    let (row, ours) = self
      .db
      .read(move |conn| {
        let row = store::get(conn, &name)?;
        let ours = blocking::stored_of(conn, &name)?;
        Ok((row, ours))
      })
      .await?;
    let ours = ours.filter(|b| blocking::stands(b.expires_at, now));
    let block = ours.or_else(|| {
      self
        .blocks
        .configuration_blocks(username)
        .then(|| blocking::configured(username))
    });
    Ok((row, block))
  }

  /// `username`'s summary, built from what is stored now.
  async fn summary_of(&self, username: &str) -> Result<UserSummary, crate::api::error::ApiError> {
    let internal = crate::api::error::ApiError::internal;
    let (row, block) = self.stored(username).await.map_err(internal)?;
    let figures = self
      .figures
      .get(&self.transfers, &self.messaging, username)
      .await
      .map_err(internal)?;
    let options = self.config.current().values.clone();
    let user = profile::known(&options, username, row.as_ref(), block);
    let user = self.with_network_and_conversation(user).await;
    let summary = profile::summary(user, row.as_ref(), &figures);
    let mut summaries = lock(&self.summaries);
    summaries.retain(|_, (at, _)| at.elapsed() < SUMMARY_FOR);
    summaries.insert(username.to_string(), (Instant::now(), summary.clone()));
    Ok(summary)
  }

  /// The country last seen for `username`, when they have a row and one was seen; read from
  /// memory.
  pub fn country_of(&self, username: &str) -> Option<String> {
    self.known.country(username)
  }

  /// Pushes `username`'s summary after their row or block changed; they have a row now.
  async fn changed(&self, username: &str) {
    self.known.added(username);
    self.forget(username);
    match self.summary_of(username).await {
      Ok(summary) => self
        .live
        .publish(LiveFrame::UsersUpdated(Box::new(summary))),
      Err(e) => warn!(username, error = ?e, "user change not pushed"),
    }
  }

  /// Pushes that `username` left the directory.
  fn removed(&self, username: &str) {
    self.known.removed(username);
    self.forget(username);
    self.live.publish(LiveFrame::UsersRemoved(UserRemoved {
      username: username.to_string(),
    }));
  }

  /// Forgets what is cached about `username`.
  fn forget(&self, username: &str) {
    self.figures.forget(username);
    lock(&self.summaries).remove(username);
  }

  /// Writes the touches as they come, pushing the users whose row changed.
  async fn write_touches(
    self: Arc<Self>,
    mut receiver: tokio::sync::mpsc::Receiver<touch::Touched>,
    shutdown: CancellationToken,
  ) {
    while let Some(batch) = touch::gather(&mut receiver, &shutdown).await {
      let count = batch.len();
      match self.db.write(move |conn| touch::write(conn, &batch)).await {
        Ok(changed) => {
          for username in changed {
            self.changed(&username).await;
          }
        }
        Err(e) => warn!(users = count, error = %e, "user touches not written"),
      }
    }
  }

  /// Every minute removes the blocks that expired; every five minutes forgets the users past
  /// `retention.users`.
  async fn sweep(self: Arc<Self>, shutdown: CancellationToken) {
    let mut expiry = tokio::time::interval(EXPIRY_SWEEP);
    let mut retention = tokio::time::interval(RETENTION_SWEEP);
    loop {
      tokio::select! {
        () = shutdown.cancelled() => return,
        _ = expiry.tick() => self.expire_blocks().await,
        _ = retention.tick() => self.forget_unseen().await,
      }
    }
  }

  /// Removes the blocks that expired, raising `UserUnblocked` for each, and forgets the
  /// remembered answers past their time.
  async fn expire_blocks(&self) {
    self.blocks.forget_stale();
    let now = Utc::now();
    let expired = match self.db.write(move |conn| blocking::expire(conn, now)).await {
      Ok(expired) => expired,
      Err(e) => {
        warn!(error = %e, "expired blocks not removed");
        return;
      }
    };
    for username in expired {
      info!(username, "block expired");
      self.blocks.unset(&username);
      self.events.raise(Event::UserUnblocked(UserUnblocked {
        version: 0,
        username: username.clone(),
        reason: UnblockReason::Expired,
        by: None,
      }));
      self.changed(&username).await;
    }
  }

  /// Forgets the users not seen for `retention.users` that have nothing of ours.
  async fn forget_unseen(&self) {
    let Some(age) = self.config.current().values.retention.users else {
      return;
    };
    // an age too long for chrono forgets no one, as the earliest time stored bounds it
    let age = chrono::Duration::from_std(age).unwrap_or(chrono::Duration::MAX);
    let before = crate::db::shifted(Utc::now(), -age);
    match self
      .db
      .write(move |conn| retention::prune(conn, before))
      .await
    {
      Ok(pruned) => {
        if !pruned.is_empty() {
          info!(count = pruned.len(), "users not seen lately forgotten");
        }
        for username in pruned {
          self.removed(&username);
        }
      }
      Err(e) => warn!(error = %e, "users not seen lately not forgotten"),
    }
  }

  /// Takes every configuration change: the blacklist is compiled again, the blocklist file
  /// read again when its settings changed, and the cached summaries dropped, since groups may
  /// have moved.
  async fn follow_configuration(self: Arc<Self>, shutdown: CancellationToken) {
    let mut changes = self.config.changes();
    loop {
      let change = tokio::select! {
        () = shutdown.cancelled() => return,
        change = changes.recv() => change,
      };
      match change {
        Ok(change) => {
          let options = &change.current.values;
          self.blocks.configure(options);
          if change
            .changed
            .iter()
            .any(|key| key.starts_with("blacklist."))
          {
            self.reload_blocklist(options).await;
          }
        }
        Err(RecvError::Lagged(missed)) => {
          debug!(
            missed,
            "configuration changes missed; taking the current values"
          );
          let options = self.config.current().values.clone();
          self.blocks.configure(&options);
          self.reload_blocklist(&options).await;
        }
        Err(RecvError::Closed) => return,
      }
      lock(&self.summaries).clear();
    }
  }

  /// Reads the blocklist file again; a failure keeps the list loaded before.
  async fn reload_blocklist(&self, options: &Options) {
    let Some(path) = blocklist_file(options) else {
      self.blocks.set_blocklist(None);
      return;
    };
    match load_blocklist(path.clone()).await {
      Ok(list) => self.blocks.set_blocklist(Some((path, list))),
      Err(e) => error!(
        path = %path.display(),
        error = e,
        "blacklist not reloaded; the list loaded before stays"
      ),
    }
  }
}

/// The blocklist file to load, when the blocklist is on.
fn blocklist_file(options: &Options) -> Option<PathBuf> {
  options
    .blacklist
    .enabled
    .then(|| options.blacklist.file.clone())
    .flatten()
}

/// Reads the blocklist file at `path` on a blocking thread, logging what it holds and how long
/// it took.
async fn load_blocklist(path: PathBuf) -> Result<Blocklist, String> {
  let started = Instant::now();
  tokio::task::spawn_blocking(move || {
    let list = Blocklist::load(&path)?;
    info!(
      count = list.count(),
      path = %path.display(),
      ms = started.elapsed().as_millis() as u64,
      "Blacklist updated"
    );
    Ok(list)
  })
  .await
  .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod testing {
  use crate::db::{Database, Location};

  /// An empty `users.db` in memory.
  pub async fn users_db() -> Database {
    let location = Location {
      data: std::env::temp_dir(),
      backups: std::env::temp_dir(),
      volatile: true,
      log_sql: false,
    };
    Database::open(&super::store::SCHEMA, &location)
      .await
      .unwrap()
  }
}
