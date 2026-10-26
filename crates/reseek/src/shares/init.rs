//! The shares' background work besides scanning: loading the index at startup or scanning
//! when it can not be trusted, the scheduled rescan, and marking a rescan pending when the
//! share options change.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast::error::RecvError;
use tracing::{Instrument, debug, error, info, info_span, warn};

use super::scanner::{self, Settings};
use super::{Shares, lock, repository, share, storage};
use crate::config::Options;
use crate::config::enums::StorageMode;

/// How often the scheduled rescan checks whether one is due, and how soon after startup it
/// first does.
const RETENTION_CHECK: Duration = Duration::from_secs(3600);
const RETENTION_FIRST: Duration = Duration::from_secs(60);

/// Starts the shares' background work for the options the daemon started with.
pub(super) fn start(shares: &Arc<Shares>, options: &Options) {
  if !options.flags.no_share_scan {
    let force = options.flags.force_share_scan;
    let mode = options.shares.cache.storage_mode;
    let engine = shares.clone();
    tokio::spawn(
      async move { engine.initialize(force, mode).await }.instrument(info_span!("shares")),
    );
  }
  let engine = shares.clone();
  tokio::spawn(async move { engine.retention().await }.instrument(info_span!("shares")));
  let engine = shares.clone();
  tokio::spawn(async move { engine.follow_options().await }.instrument(info_span!("shares")));
}

impl Shares {
  /// Loads the index, or scans when it can not be trusted. A loaded index is served at once:
  /// the shares are watched, the engine is marked initialized, and one scan reusing the loaded
  /// rows runs in the background to find what changed while reseek was not running. Otherwise
  /// the engine is marked initialized once the scan is done.
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
    } else {
      match self.load(mode).await {
        Ok(settings) => {
          self.watch_shares(settings).await;
          // a conflict means a scan asked for over the API already runs and covers the same
          // ground
          if self.scan(false).is_err() {
            debug!("a share scan already runs; the startup scan is left to it");
          }
          self.initialized.send_replace(true);
          return;
        }
        Err(e) => {
          warn!(error = e, "Error initializing shares");
          info!("Re-attempting initialization");
          // the index that failed to load is not trusted, so no row of it is reused
          if !self.scan_at_startup(true).await {
            error!(
              "Failed to initialize shares, and an attempt to force a full scan to repair failed. \
               Sharing is disabled"
            );
          }
        }
      }
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
      .run_scan(force, cancel)
      .instrument(info_span!("scan", force))
      .await
  }

  /// Loads the index from its backup or its file and checks the last scan it records covers
  /// the shares as configured now; says why when it can not be used.
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
    let options = self.config.current();
    let settings = Arc::new(Settings::new(self.listed(), &options.values));
    if scanner::content(&latest.options) != settings.snapshot.get("content").cloned() {
      return Err("Share options changed since previous scan".into());
    }
    let totals = self
      .count_shares(&settings.shares)
      .await
      .map_err(|e| e.to_string())?;
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
    self.counts.send_replace(totals);
    super::browse::refresh(&self.db, &self.browse_cache()).await;
    Ok(settings)
  }

  /// Every hour, scans when `shares.cache.retention` is set and no complete, trusted scan
  /// started within it.
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

  /// Starts a scan when `shares.cache.retention` is set and no complete, trusted scan started
  /// within it; true when one was started.
  pub(super) async fn retention_tick(self: &Arc<Self>) -> bool {
    if lock(&self.status).scanning {
      return false;
    }
    let Some(retention) = self.config.current().values.shares.cache.retention else {
      return false;
    };
    // a retention too long for the clock's milliseconds reaches back past every scan
    let cutoff = scanner::now_ms() - i64::try_from(retention.as_millis()).unwrap_or(i64::MAX);
    let scans = match self
      .db
      .read(move |conn| repository::scans_since(conn, cutoff))
      .await
    {
      Ok(scans) => scans,
      Err(e) => {
        warn!(error = %e, "scheduled share rescan not checked");
        return false;
      }
    };
    if scans.iter().any(|s| s.ended_at.is_some() && !s.suspect) {
      return false;
    }
    info!(
      minutes = retention.as_secs() / 60,
      "Beginning scheduled re-scan of shares (previous scan is older than the retention)"
    );
    match self.scan(false) {
      Ok(()) => true,
      Err(e) => {
        info!(?e, "scheduled share rescan not started");
        false
      }
    }
  }

  /// Marks a rescan pending when the shared directories, the filters or link following
  /// change; a removed share stops being watched at once.
  async fn follow_options(self: &Arc<Self>) {
    let mut changes = self.config.changes();
    loop {
      let change = tokio::select! {
        () = self.shutdown.cancelled() => return,
        change = changes.recv() => change,
      };
      let current = match change {
        Ok(change) => change.current.values.clone(),
        // the options now are compared with those last applied, so a missed change is caught
        Err(RecvError::Lagged(_)) => self.config.current().values.clone(),
        Err(RecvError::Closed) => return,
      };
      self.apply_options(&current);
    }
  }

  /// Applies the share options of `current` where they differ from those last applied.
  pub(super) fn apply_options(&self, current: &Options) {
    let previous = std::mem::replace(&mut *lock(&self.applied), current.shares.clone());
    let list = share::configure(&current.shares, &self.downloads);
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
    if scanner::filter_set(&previous.filters) != scanner::filter_set(&current.shares.filters) {
      info!(
        "File filter configuration changed.  Shares must be re-scanned for changes to take effect."
      );
      pending = true;
    }
    if previous.follow_symlinks != current.shares.follow_symlinks {
      info!(
        "Link following configuration changed.  Shares must be re-scanned for changes to take effect."
      );
      pending = true;
    }
    if pending {
      self.publish(|status| status.scan_pending = true);
    }
  }
}
