//! The directory of users we dealt with: the row kept for each in `users.db`, touched as we
//! deal with them, our notes, tags, pins, mutes and groups for them, the figures of their
//! transfers, and who is blocked from what.
//!
//! [`Directory`] is the one engine: the API's users directory and blocking calls, and the two
//! calls the network engines make, [`Directory::touch`] and [`Directory::is_blocked`].

mod blocking;
mod blocklist;
mod groups;
mod listing;
mod profile;
mod retention;
mod service;
mod store;
#[cfg(test)]
mod tests;
mod touch;
mod transfers;

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use api_types::blocking::{Block, BlockScope};
use api_types::events::{Event, UnblockReason, UserUnblocked};
use api_types::live::LiveFrame;
use api_types::users::{UserRemoved, UserSummary};
use cascade::Watcher;
use chrono::Utc;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, info_span, warn};

pub use touch::{PeerInfo, Touch};

use crate::config::Options;
use crate::db::{Database, DbError, Location};
use crate::events::EventBus;
use crate::live::Live;
use blocking::Blocks;
use blocklist::Blocklist;
use store::Stored;
use touch::Toucher;
use transfers::FigureCache;

/// How long a summary is served again without being built anew.
const SUMMARY_FOR: Duration = Duration::from_secs(60);
/// How often expired blocks are removed.
const EXPIRY_SWEEP: Duration = Duration::from_secs(60);
/// How often users past `retention.users` are forgotten.
const RETENTION_SWEEP: Duration = Duration::from_secs(300);

/// The users directory and the blocks.
pub struct Directory {
  db: Database,
  transfers: Database,
  config: Watcher<Options>,
  live: Live,
  events: EventBus,
  toucher: Toucher,
  blocks: Blocks,
  figures: FigureCache,
  summaries: Mutex<HashMap<String, (Instant, UserSummary)>>,
}

impl Directory {
  /// Opens `users.db` in `location` and loads the blocks and the blocklist file, then starts
  /// the touch writer, the expiry and retention sweeps and the follower of the configuration,
  /// which all stop when `shutdown` is cancelled.
  pub async fn open(
    location: &Location,
    transfers: Database,
    config: Watcher<Options>,
    live: Live,
    events: EventBus,
    shutdown: CancellationToken,
  ) -> Result<Arc<Self>, String> {
    let db = Database::open(&store::SCHEMA, location)
      .await
      .map_err(|e| format!("users.db: {e}"))?;
    let ours = db
      .read(|conn| blocking::stored(conn, Utc::now()))
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
      info!(count = list.count(), path = %path.display(), "Blacklist updated");
      blocks.set_blocklist(Some((path, list)));
    }
    let (toucher, receiver) = Toucher::channel();
    let directory = Arc::new(Self {
      db,
      transfers,
      config,
      live,
      events,
      toucher,
      blocks,
      figures: FigureCache::default(),
      summaries: Mutex::new(HashMap::new()),
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
      .get(&self.transfers, username)
      .await
      .map_err(internal)?;
    let options = self.config.current().values.clone();
    let user = profile::known(&options, username, row.as_ref(), block);
    let summary = profile::summary(user, row.as_ref(), &figures);
    self
      .summaries
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .insert(username.to_string(), (Instant::now(), summary.clone()));
    Ok(summary)
  }

  /// Pushes `username`'s summary after their row or block changed.
  async fn changed(&self, username: &str) {
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
    self.forget(username);
    self.live.publish(LiveFrame::UsersRemoved(UserRemoved {
      username: username.to_string(),
    }));
  }

  /// Forgets what is cached about `username`.
  fn forget(&self, username: &str) {
    self.figures.forget(username);
    self
      .summaries
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .remove(username);
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
      self
        .summaries
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    }
  }

  /// Reads the blocklist file again; a failure keeps the list loaded before.
  async fn reload_blocklist(&self, options: &Options) {
    let Some(path) = blocklist_file(options) else {
      self.blocks.set_blocklist(None);
      return;
    };
    match load_blocklist(path.clone()).await {
      Ok(list) => {
        info!(count = list.count(), path = %path.display(), "Blacklist updated");
        self.blocks.set_blocklist(Some((path, list)));
      }
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

async fn load_blocklist(path: PathBuf) -> Result<Blocklist, String> {
  tokio::task::spawn_blocking(move || Blocklist::load(&path))
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
