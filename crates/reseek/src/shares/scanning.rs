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
    let options = self.config.current();
    let settings = Arc::new(Settings::new(self.listed(), &options.values));
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
          Ok(totals) => {
            *lock(&self.covered) = Some(settings.clone());
            let current = self.covers_current(&settings);
            if !current {
              debug!("the share options changed while the scan ran, so a rescan stays pending");
            }
            self.publish(|status| {
              status.scanning = false;
              status.faulted = false;
              status.canceled = false;
              if current {
                status.scan_pending = false;
              }
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
            *lock(&self.local_counts) = totals;
            self.counts.send_replace(self.with_agents(totals));
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
    browse::refresh(&self.databases(), &self.browse_cache()).await;
    *lock(&self.browse_throttle.last) = Some(Instant::now());
    if completed {
      self.watch_shares(settings).await;
    }
    completed
  }

  /// Whether `settings` share what a scan started now would: the same entries, filters and
  /// link following.
  fn covers_current(&self, settings: &Settings) -> bool {
    let options = self.config.current();
    settings.snapshot.get("content") == Some(&scanner::content_of(&self.listed(), &options.values))
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
