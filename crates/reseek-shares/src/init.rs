//! The shares' background work besides scanning: loading the index at startup or scanning
//! when it cannot be trusted, the scheduled rescan, and marking a rescan pending when the
//! share options change.

use std::sync::Arc;
use std::time::Duration;

use cascade::{Change, Watcher};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tracing::{Instrument, debug, error, info, info_span, warn};

use super::scanner::{self, Settings};
use super::schedule::Schedule;
use super::{ScanOrigin, Shares, repository, share, storage};
use crate::db::DbError;
use crate::lock;
use crate::options::{self, StorageMode};
use chrono::{DateTime, Local, Utc};

/// How often the scheduled rescan checks whether one is due, and how soon after startup it
/// first does.
const RETENTION_CHECK: Duration = Duration::from_secs(3600);
const RETENTION_FIRST: Duration = Duration::from_secs(60);

/// Starts the shares' background work: the startup load or scan when `scan` asks for it, a
/// full one with `force`, and following the share options `select` finds in `config`.
pub(super) fn start<O: cascade::Root>(
  shares: &Arc<Shares>,
  config: &Watcher<O>,
  select: fn(&O) -> &options::Shares,
  scan: bool,
  force: bool,
  rescan_on_schedule: bool,
) {
  if scan {
    let mode = select(&config.current().values).cache.storage_mode;
    let engine = shares.clone();
    tokio::spawn(
      async move { engine.initialize(force, mode).await }.instrument(info_span!("shares")),
    );
  }
  if rescan_on_schedule {
    let engine = shares.clone();
    tokio::spawn(async move { engine.retention().await }.instrument(info_span!("shares")));
  }
  let engine = shares.clone();
  let changes = config.changes();
  tokio::spawn(
    async move { engine.follow_options(changes, select).await }.instrument(info_span!("shares")),
  );
}

impl Shares {
  /// Loads the index, or scans when it cannot be trusted. A loaded index is served at once:
  /// the shares are watched, the engine is marked initialized, and one scan reusing the loaded
  /// rows runs in the background to find what changed while reseek was not running. Otherwise
  /// the engine is marked initialized once the scan is done. The load holds the scan slot, and
  /// a scan that holds it first is left to fill the index.
  async fn initialize(self: &Arc<Self>, force: bool, mode: StorageMode) {
    info!("Initializing shares");
    self.publish(|status| status.ready = false);
    // the scan logs why it failed; these say that sharing is off because of it
    if force {
      warn!("Performing a forced re-scan of shares");
      if !self.scan_at_startup(true).await {
        error!("Failed to initialize shares. Sharing is disabled");
      }
    } else if self.in_memory && self.backup.is_none() {
      debug!("the share index is kept only in memory, so it starts with a scan");
      if !self.scan_at_startup(false).await {
        error!("Failed to initialize shares. Sharing is disabled");
      }
    } else if let Some(cancel) = self.take_slot() {
      // the load holds the scan slot, so no scan writes the index or the state under it
      match self.load(mode).await {
        Ok(settings) => {
          self.watch_shares(settings).await;
          self.initialized.send_replace(true);
          let shares = self.clone();
          tokio::spawn(
            async move { shares.run_scan(false, ScanOrigin::Startup, cancel).await }
              .instrument(info_span!("scan", force = false)),
          );
          return;
        }
        Err(e) => {
          warn!(error = e, "Error initializing shares");
          info!("Re-attempting initialization");
          // the index that failed to load is not trusted, so no row of it is reused
          let repaired = self
            .run_scan(true, ScanOrigin::Startup, cancel)
            .instrument(info_span!("scan", force = true))
            .await;
          if !repaired {
            error!(
              "Failed to initialize shares, and an attempt to force a full scan to repair failed. \
               Sharing is disabled"
            );
          }
        }
      }
    } else {
      debug!("a share scan already runs; loading the index is left to it");
    }
    self.initialized.send_replace(true);
  }

  /// A scan run as part of starting up, waited for; true when it completed, or when a scan
  /// asked for over the API holds the slot and the startup scan is left to it.
  pub(super) async fn scan_at_startup(self: &Arc<Self>, force: bool) -> bool {
    let Some(cancel) = self.take_slot() else {
      debug!("a share scan already runs; the startup scan is left to it");
      return true;
    };
    self
      .run_scan(force, ScanOrigin::Startup, cancel)
      .instrument(info_span!("scan", force))
      .await
  }

  /// Loads the index from its backup or its file and checks the last scan it records covers
  /// the shares as configured now; says why when it cannot be used.
  async fn load(&self, mode: StorageMode) -> Result<Arc<Settings>, String> {
    if self.in_memory {
      info!("Share cache StorageMode is 'Memory'. Attempting to load from backup...");
      let backup = self
        .backup
        .clone()
        .ok_or("Share cache backup is missing, corrupt, or is out of date")?;
      let checked = backup.clone();
      tokio::task::spawn_blocking(move || storage::valid(&checked))
        .await
        .map_err(|e| format!("Share cache backup was not checked: {e}"))?
        .map_err(|e| format!("Share cache backup is missing, corrupt, or is out of date: {e}"))?;
      info!("Share cache backup validated. Attempting to restore...");
      storage::restore(&self.db, &backup)
        .await
        .map_err(|e| format!("Share cache backup could not be restored: {e}"))?;
      info!("Share cache successfully restored from backup");
    } else if mode == StorageMode::Disk {
      info!("Share cache StorageMode is 'Disk'. Attempting to validate...");
    }
    let latest = self
      .db
      .read(repository::latest_scan)
      .await
      .map_err(|e| e.to_string())?
      .ok_or("Shares not yet scanned")?;
    if latest.ended_at.is_none() {
      return Err("Previous share scan did not complete".into());
    }
    if latest.suspect {
      return Err("Previous share scan was marked as suspect".into());
    }
    let settings = Arc::new(self.scan_settings());
    let recorded = scanner::content(&latest.options)
      .map_err(|e| format!("Previous share scan recorded options that cannot be read: {e}"))?;
    if recorded != settings.snapshot.get("content").cloned() {
      return Err("Share options changed since previous scan".into());
    }
    let counted = self
      .count_shares(&settings.shares)
      .await
      .map_err(|e| e.to_string())?;
    let totals = counted.total();
    *lock(&self.covered) = Some(settings.clone());
    self.publish(|status| {
      status.scanning = false;
      status.faulted = false;
      status.ready = true;
      status.loaded = true;
      status.scan_pending = false;
      status.progress = 1.0;
      status.directories = totals.directories;
      status.files = totals.files;
    });
    info!(
      directories = totals.directories,
      files = totals.files,
      "Share cache loaded from disk successfully"
    );
    *lock(&self.local_counts) = counted;
    self.counts.send_replace(self.server_counts());
    self.changed.send_modify(|n| *n += 1);
    self.check_unmatched();
    self.refresh_browse_caches().await;
    Ok(settings)
  }

  /// Every hour, scans when `shares.cache.retention` finds a scan due.
  async fn retention(self: &Arc<Self>) {
    let start = tokio::time::Instant::now() + RETENTION_FIRST;
    let mut every = tokio::time::interval_at(start, RETENTION_CHECK);
    loop {
      tokio::select! {
        () = self.shutdown.cancelled() => return,
        _ = every.tick() => {}
      }
      self.retention_tick().await;
    }
  }

  /// Starts a scan when `shares.cache.retention` finds one due after the last complete,
  /// trusted scan; true when one was started.
  pub(super) async fn retention_tick(self: &Arc<Self>) -> bool {
    if lock(&self.status).scanning {
      return false;
    }
    let schedule = (self.options)().cache.retention;
    if schedule == Schedule::Off {
      return false;
    }
    let last = match self.last_trusted_scan().await {
      Ok(last) => last,
      Err(e) => {
        warn!(error = %e, "scheduled share rescan not checked");
        return false;
      }
    };
    let now = Utc::now();
    if schedule.next(last, now, &Local).is_none_or(|due| due > now) {
      return false;
    }
    info!(%schedule, "Beginning scheduled re-scan of shares (previous scan is older than the retention)");
    match self.scan(false, ScanOrigin::Scheduled) {
      Ok(()) => true,
      Err(e) => {
        info!(?e, "scheduled share rescan not started");
        false
      }
    }
  }

  /// When the last scan that completed and is trusted started, none when there is none.
  pub async fn last_trusted_scan(&self) -> Result<Option<DateTime<Utc>>, DbError> {
    let started = self.db.read(repository::latest_trusted_scan).await?;
    // the scan wrote its start from the clock, which is always within the range of a time
    Ok(started.and_then(DateTime::from_timestamp_millis))
  }

  /// Marks a rescan pending when the shared directories, the filters or link following
  /// change; a removed share stops being watched at once.
  async fn follow_options<O: cascade::Root>(
    self: &Arc<Self>,
    mut changes: broadcast::Receiver<Arc<Change<O>>>,
    select: fn(&O) -> &options::Shares,
  ) {
    loop {
      let change = tokio::select! {
        () = self.shutdown.cancelled() => return,
        change = changes.recv() => change,
      };
      let current = match change {
        Ok(change) => select(&change.current.values).clone(),
        // the options now are compared with those last applied, so a missed change is caught
        Err(RecvError::Lagged(_)) => (self.options)(),
        Err(RecvError::Closed) => return,
      };
      self.apply_options(&current);
    }
  }

  /// Applies the share options `current` where they differ from those last applied.
  pub(super) fn apply_options(&self, current: &options::Shares) {
    let previous = std::mem::replace(&mut *lock(&self.applied), current.clone());
    let list = share::configure(current, &self.downloads);
    let changed_list = {
      let mut shares = lock(&self.shares);
      let changed = share::fingerprint(&shares) != share::fingerprint(&list);
      if changed {
        *shares = list.clone();
      }
      changed
    };
    let mut pending = false;
    if changed_list {
      info!(
        "Shared directory configuration changed.  Shares must be re-scanned for changes to take effect."
      );
      pending = true;
      if let Some(watching) = &mut *lock(&self.watches) {
        watching.keep_only(&list);
      }
    }
    if scanner::filter_set(&previous.filters) != scanner::filter_set(&current.filters) {
      info!(
        "File filter configuration changed.  Shares must be re-scanned for changes to take effect."
      );
      pending = true;
    }
    if previous.visible_to != current.visible_to {
      info!("Share visibility changed");
      self.recount_for_server();
    }
    if previous.follow_symlinks != current.follow_symlinks {
      info!(
        "Link following configuration changed.  Shares must be re-scanned for changes to take effect."
      );
      pending = true;
    }
    if pending {
      self.publish(|status| status.scan_pending = true);
    }
    self.check_unmatched();
  }
}
