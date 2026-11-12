//! A scan's life in the engine: the one scan slot, starting and canceling a scan, running it
//! with its outcome put into the state, and the work that follows a scan or a watched change
//! at most once a minute.

use super::*;

impl Shares {
  /// Starts a scan in the background, reading every file again with `force`, told to
  /// [`Setup::scans`](crate::Setup::scans) as started by `origin`.
  pub fn scan(&self, force: bool, origin: ScanOrigin) -> Result<(), ScanConflict> {
    let shares = self
      .this
      .upgrade()
      .expect("the engine is only made inside the Arc it refers to");
    let Some(cancel) = self.take_slot(Edge::Scan) else {
      // while the index loads, the scan that follows the load is the one asked for
      return match self.transition(Edge::Join { force, origin }) {
        Some(_) => Ok(()),
        None => Err(ScanConflict::Running),
      };
    };
    // the scan outlives whatever asked for it, such as a request, so its span stands alone
    tokio::spawn(
      async move { shares.run_scan(force, origin, cancel).await }
        .instrument(info_span!(parent: None, "scan", force)),
    );
    Ok(())
  }

  /// Takes the one scan slot with `take`, a load or a scan, returning the token that cancels
  /// the scan; `None` while a load or a scan holds it.
  pub(super) fn take_slot(&self, take: fn(CancellationToken) -> Edge) -> Option<CancellationToken> {
    let cancel = self.shutdown.child_token();
    self.transition(take(cancel.clone())).map(|_| cancel)
  }

  /// Takes `edge`, which only the load or the scan holding the slot takes, and always can;
  /// returns the phase it left.
  pub(super) fn hold(&self, edge: Edge) -> Phase {
    self
      .transition(edge)
      .expect("the holder of the scan slot moves its own phase")
  }

  /// Asks the running scan to stop; a scan that ended while its end is still being told needs
  /// nothing more.
  pub fn cancel_scan(&self) -> Result<(), ScanConflict> {
    let status = lock(&self.status);
    match status.phase.cancel() {
      Some(cancel) => {
        cancel.cancel();
        Ok(())
      }
      None if status.phase.holds_slot() => {
        debug!("the share scan asked to stop has already ended");
        Ok(())
      }
      None => Err(ScanConflict::NotRunning),
    }
  }

  /// Runs a scan, telling the hook as it starts and ends, and frees the slot once the hook was
  /// told of the end. It goes by the shares as configured once the hook heard of its start.
  pub(super) async fn run_scan(
    self: &Arc<Self>,
    force: bool,
    origin: ScanOrigin,
    cancel: CancellationToken,
  ) -> bool {
    let mut slot = HeldSlot {
      shares: self.clone(),
      told_end: false,
      released: false,
    };
    (self.scans)(ScanNotice::Started { force, origin }).await;
    let settings = Arc::new(self.scan_settings());
    let ended = self.scan_now(force, cancel, settings).await;
    let completed = matches!(ended, ScanEnded::Completed(_));
    // the hook hears of the end while the slot is held, so no scan starts between the two
    slot.told_end = true;
    (self.scans)(ScanNotice::Ended(ended)).await;
    slot.release();
    completed
  }

  /// One scan of the configured shares, its outcome put into the state.
  async fn scan_now(
    self: &Arc<Self>,
    force: bool,
    cancel: CancellationToken,
    settings: Arc<Settings>,
  ) -> ScanEnded {
    let started = Instant::now();
    info!("Share scan started");
    let reporter = {
      let shares = self.clone();
      move |progress: scanner::Progress| {
        let percent = |p: f64| (p * 100.0).round() as u32;
        let changed = match &lock(&shares.status).phase {
          Phase::Scanning {
            progress: shown, ..
          } => {
            percent(shown.fraction) != percent(progress.fraction)
              || shown.directories != progress.directories
          }
          _ => false,
        };
        if changed {
          debug!(
            percent = percent(progress.fraction),
            files = progress.files,
            "Scanned shared directories"
          );
          shares.hold(Edge::Progress(progress));
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
            self.settle_pending(settings.clone()).await;
            self.hold(Edge::ScanEnded(ScanResult::Completed(totals)));
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
            ScanEnded::Completed(totals)
          }
          Err(e) => {
            self.faulted(&e.to_string());
            ScanEnded::Failed(e.to_string())
          }
        }
      }
      Ok(Outcome::Canceled) => {
        self.hold(Edge::ScanEnded(ScanResult::Canceled));
        ScanEnded::Canceled
      }
      Err(e) => {
        self.faulted(&e.to_string());
        ScanEnded::Failed(e.to_string())
      }
    };
    self.check_unmatched();
    self.refresh_browse_caches().await;
    *lock(&self.browse_throttle.last) = Some(Instant::now());
    if matches!(completed, ScanEnded::Completed(_)) {
      self.watch_shares(settings).await;
    }
    completed
  }

  /// Clears the pending rescan after a scan of `settings` that completed, unless the share
  /// options no longer match them or a drift from the disk marked its row. The row is read,
  /// the options compared and the flag cleared in one write under the state's lock, which a
  /// drift's mark and an option change's flag are ordered against.
  pub(super) async fn settle_pending(self: &Arc<Self>, settings: Arc<Settings>) {
    let shares = self.clone();
    let settled = self
      .db
      .write(move |conn| {
        let suspect = repository::latest_scan(conn)?.is_some_and(|scan| scan.suspect);
        let mut status = lock(&shares.status);
        let current = shares.covers_current(&settings);
        if current && !suspect {
          status.scan_pending = false;
        }
        Ok((current, suspect))
      })
      .await;
    match settled {
      Ok((_, true)) => {
        debug!("a drift from the disk was reported while the scan ran, so a rescan stays pending")
      }
      Ok((false, false)) => {
        debug!("the share options changed while the scan ran, so a rescan stays pending")
      }
      Ok((true, false)) => {}
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
  pub(super) fn faulted(&self, error: &str) {
    tracing::error!(error, "Failed to scan shares.");
    self.hold(Edge::ScanEnded(ScanResult::Failed));
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

/// Why a scan that stopped while it ran failed.
const STOPPED: &str = "the scan stopped before it ended";

/// The scan slot as a running scan holds it, freed by [`HeldSlot::release`] once the hook heard
/// of the scan's end. Dropped before that, by a panic or a dropped future, it ends a scan that
/// still ran as failed, tells the hook the end it was not yet told, then frees the slot, so
/// later scans can still start. Dropped as the engine shuts down, it does nothing: the next
/// start ends the scan's row.
struct HeldSlot {
  shares: Arc<Shares>,
  /// the end was handed to the hook
  told_end: bool,
  released: bool,
}

impl HeldSlot {
  fn release(mut self) {
    self.released = true;
    self.shares.hold(Edge::Release);
  }
}

/// The end the hook is told of a scan that ended as `ended`, serving `served`.
fn told_as(ended: phase::Ended, served: Option<SharedCounts>) -> ScanEnded {
  match ended {
    // a completed scan always serves its totals
    phase::Ended::Completed => ScanEnded::Completed(served.unwrap_or_default()),
    phase::Ended::Canceled => ScanEnded::Canceled,
    phase::Ended::Failed => ScanEnded::Failed("the scan failed; the log says why".to_string()),
  }
}

impl Drop for HeldSlot {
  fn drop(&mut self) {
    if self.released {
      return;
    }
    let shares = self.shares.clone();
    if shares.shutdown.is_cancelled() {
      return debug!("a share scan stopped with the engine; the next start ends its row");
    }
    if self.told_end {
      warn!("the scan hook did not finish hearing of the scan's end; the scan slot is freed");
      shares.hold(Edge::Release);
      return;
    }
    let phase = lock(&shares.status).phase.clone();
    let ended = match phase {
      Phase::Ending { ended, served } => {
        warn!("a share scan stopped after it ended, before the scan hook heard of the end");
        told_as(ended, served)
      }
      // the slot is held, so the scan still ran
      _ => {
        shares.faulted(STOPPED);
        ScanEnded::Failed(STOPPED.to_string())
      }
    };
    match tokio::runtime::Handle::try_current() {
      // the hook hears of the end before the slot is free, as after any scan; the task runs
      // on its own
      Ok(runtime) => drop(
        runtime.spawn(
          async move {
            (shares.scans)(ScanNotice::Ended(ended)).await;
            shares.hold(Edge::Release);
          }
          .in_current_span(),
        ),
      ),
      // with no runtime left the hook cannot be awaited, so only the slot is freed
      Err(_) => {
        shares.hold(Edge::Release);
      }
    }
  }
}
