//! A scan's life in the engine: the one scan slot, starting and canceling a scan, running it
//! with its outcome put into the state, and the work that follows a scan or a watched change
//! at most once a minute.

use super::*;

impl Shares {
  /// Starts a scan in the background, reading every file again with `force`.
  pub fn scan(&self, force: bool) -> Result<(), ScanConflict> {
    let shares = self
      .this
      .upgrade()
      .expect("the engine is only made inside the Arc it refers to");
    let cancel = self.take_slot().ok_or(ScanConflict::Running)?;
    tokio::spawn(
      async move { shares.run_scan(force, cancel).await }.instrument(info_span!("scan", force)),
    );
    Ok(())
  }

  /// Takes the one scan slot, with the token that cancels the scan; `None` while a scan holds
  /// it.
  pub(super) fn take_slot(&self) -> Option<CancellationToken> {
    let mut running = lock(&self.running);
    if running.is_some() {
      return None;
    }
    let cancel = self.shutdown.child_token();
    *running = Some(cancel.clone());
    Some(cancel)
  }

  /// Asks the running scan to stop.
  pub fn cancel_scan(&self) -> Result<(), ScanConflict> {
    match &*lock(&self.running) {
      Some(cancel) => {
        cancel.cancel();
        Ok(())
      }
      None => Err(ScanConflict::NotRunning),
    }
  }

  /// Runs a scan whose slot is taken, and frees it at the end.
  pub(super) async fn run_scan(self: &Arc<Self>, force: bool, cancel: CancellationToken) -> bool {
    let completed = self.scan_now(force, cancel).await;
    *lock(&self.running) = None;
    completed
  }

  /// One scan of the configured shares, its outcome put into the state; true when it
  /// completed.
  async fn scan_now(self: &Arc<Self>, force: bool, cancel: CancellationToken) -> bool {
    let started = Instant::now();
    // a scan that does not complete leaves the index served before it, so its state stays
    let before = lock(&self.status).clone();
    self.publish(|status| {
      status.scanning = true;
      status.ready = false;
      status.canceled = false;
      status.progress = 0.0;
      status.directories = 0;
      status.files = 0;
    });
    info!("Share scan started");
    let settings = Arc::new(self.scan_settings());
    let reporter = {
      let shares = self.clone();
      move |progress: scanner::Progress| {
        let percent = |p: f64| (p * 100.0).round() as u32;
        let changed = {
          let status = lock(&shares.status);
          percent(status.progress) != percent(progress.fraction)
            || status.directories != progress.directories
        };
        if changed {
          debug!(
            percent = percent(progress.fraction),
            files = progress.files,
            "Scanned shared directories"
          );
          shares.publish(|status| {
            status.progress = progress.fraction;
            status.directories = progress.directories;
            status.files = progress.files;
          });
        }
      }
    };
    let outcome = scanner::scan(
      &self.db,
      settings.clone(),
      force,
      self.workers,
      cancel,
      reporter,
    )
    .await;
    let completed = match outcome {
      Ok(Outcome::Completed { .. }) => {
        if let Some(backup) = &self.backup
          && let Err(e) = storage::back_up(&self.db, backup).await
        {
          warn!(error = %e, "shared file cache not backed up");
        }
        match self.count_shares(&settings.shares).await {
          Ok(counted) => {
            let totals = counted.total();
            *lock(&self.covered) = Some(settings.clone());
            let current = self.covers_current(&settings);
            if !current {
              debug!("the share options changed while the scan ran, so a rescan stays pending");
            }
            self.settle_pending(current).await;
            self.publish(|status| {
              status.scanning = false;
              status.faulted = false;
              status.canceled = false;
              status.ready = true;
              status.loaded = true;
              status.progress = 1.0;
              status.directories = totals.directories;
              status.files = totals.files;
            });
            info!(
              directories = totals.directories,
              files = totals.files,
              ms = started.elapsed().as_millis() as u64,
              "Shares scanned successfully"
            );
            *lock(&self.local_counts) = counted;
            self.counts.send_replace(self.server_counts());
            *lock(&self.counts_throttle.last) = Some(Instant::now());
            self.changed.send_modify(|n| *n += 1);
            true
          }
          Err(e) => {
            self.faulted(&e.to_string(), &before);
            false
          }
        }
      }
      Ok(Outcome::Canceled) => {
        self.publish(|status| {
          status.scanning = false;
          status.canceled = true;
          status.faulted = false;
          status.progress = 1.0;
          status.scan_pending = true;
          status.ready = before.ready;
          status.directories = before.directories;
          status.files = before.files;
        });
        false
      }
      Err(e) => {
        self.faulted(&e.to_string(), &before);
        false
      }
    };
    self.check_unmatched();
    self.refresh_browse_caches().await;
    *lock(&self.browse_throttle.last) = Some(Instant::now());
    if completed {
      self.watch_shares(settings).await;
    }
    completed
  }

  /// Clears the pending rescan after a scan that completed, unless the options changed since it
  /// started (`current` false) or a drift from the disk marked its row. The row is read and
  /// the flag cleared in one write, which a drift's mark and flag are ordered against.
  async fn settle_pending(&self, current: bool) {
    let status = self.status.clone();
    let settled = self
      .db
      .write(move |conn| {
        let suspect = repository::latest_scan(conn)?.is_some_and(|scan| scan.suspect);
        if current && !suspect {
          lock(&status).scan_pending = false;
        }
        Ok(suspect)
      })
      .await;
    match settled {
      Ok(true) => {
        debug!("a drift from the disk was reported while the scan ran, so a rescan stays pending")
      }
      Ok(false) => {}
      Err(e) => {
        warn!(error = %e, "the scan's row not read again; the pending rescan is left as it was")
      }
    }
  }

  /// Whether `settings` share what a scan started now would: the same entries, filters and
  /// link following.
  fn covers_current(&self, settings: &Settings) -> bool {
    let content = scanner::content_of(&self.listed(), &(self.options)(), self.case_sensitive);
    settings.snapshot.get("content") == Some(&content)
  }

  /// Puts a failed scan into the state: the cache served is what was there before.
  pub(super) fn faulted(&self, error: &str, before: &Status) {
    tracing::error!(error, "Failed to scan shares.");
    self.publish(|status| {
      status.scanning = false;
      status.faulted = true;
      status.scan_pending = true;
      status.ready = before.ready;
      status.directories = before.directories;
      status.files = before.files;
      status.progress = 0.0;
    });
  }

  /// Runs `action` now, or once a minute after it last ran when that is later; a run already
  /// waiting takes this one's place.
  pub(super) fn throttled<F, Fut>(self: &Arc<Self>, throttle: fn(&Self) -> &Throttle, action: F)
  where
    F: FnOnce(Arc<Self>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
  {
    if throttle(self).pending.swap(true, Ordering::AcqRel) {
      return;
    }
    let shares = self.clone();
    tokio::spawn(
      async move {
        let wait = lock(&throttle(&shares).last)
          .map(|last| AT_MOST_EVERY.saturating_sub(last.elapsed()))
          .unwrap_or_default();
        tokio::select! {
          () = shares.shutdown.cancelled() => return,
          () = tokio::time::sleep(wait) => {}
        }
        throttle(&shares).pending.store(false, Ordering::Release);
        *lock(&throttle(&shares).last) = Some(Instant::now());
        action(shares).await;
      }
      .in_current_span(),
    );
  }

  /// Starts watching the shares `settings` covers, in place of what was watched before.
  pub(super) async fn watch_shares(self: &Arc<Self>, settings: Arc<Settings>) {
    if self.shutdown.is_cancelled() {
      return;
    }
    let watching = watcher::start(self.clone(), settings).await;
    *lock(&self.watches) = watching;
  }
}
