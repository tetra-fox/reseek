//! Keeping the server connection up. The watchdog runs from the start and after a connection
//! is lost, and stops once the client is connected, when asked to disconnect, and at shutdown;
//! a refused login or a disconnect asked for does not start it again. One connect loop runs at a
//! time: the first attempt at once, then each after an exponential back-off with jitter, or,
//! after a login the server never answered, 15 minutes, an hour and then 6 hours apart. A timer
//! starts a new loop while the watchdog is on and none runs. The back-off goes on across loops
//! until a login succeeds or the watchdog is restarted, so a server that drops us during every
//! login is not tried again at once each time.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_types::state::ConnectionWatchdogState;
use cascade::Watcher;
use chrono::Utc;
use reseek_shares::{backoff, lock};
use soulseek_client::{Client, ClientError};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, info_span, warn};

use crate::config::Options;
use crate::shares::Shares;
use crate::state::StateOwner;

/// How often the timer starts a loop while the watchdog is on and no loop runs.
const BACKSTOP: Duration = Duration::from_millis(100);

/// The longest wait between two attempts after transport failures.
pub const BACKOFF_MAX: Duration = Duration::from_secs(300);

/// The waits after one, two, and three or more logins in a row the server did not answer.
const UNANSWERED: [Duration; 3] = [
  Duration::from_secs(15 * 60),
  Duration::from_secs(60 * 60),
  Duration::from_secs(6 * 60 * 60),
];

/// The wait after `unanswered` logins in a row went unanswered, `None` after none.
pub fn unanswered_wait(unanswered: u32) -> Option<Duration> {
  let index = usize::try_from(unanswered.checked_sub(1)?).unwrap_or(usize::MAX);
  Some(UNANSWERED[index.min(UNANSWERED.len() - 1)])
}

/// The connect loop and its timer. Cloning gives another handle to the same watchdog.
#[derive(Clone)]
pub struct Watchdog {
  inner: Arc<Inner>,
}

struct Inner {
  client: Client,
  config: Watcher<Options>,
  state: StateOwner,
  shares: Arc<Shares>,
  shutdown: CancellationToken,
  /// the watchdog is on: the timer runs and the loop keeps trying
  enabled: watch::Sender<bool>,
  /// held by the loop that runs
  running: Arc<tokio::sync::Mutex<()>>,
  /// cancels the running loop's wait or attempt
  attempt: Mutex<Option<CancellationToken>>,
  /// attempts that failed since the last login or restart
  failures: AtomicU32,
  /// logins in a row the server left unanswered
  unanswered: AtomicU32,
  /// whether a loop runs and when its next attempt starts
  status: Mutex<Status>,
}

#[derive(Clone, Copy, Default)]
struct Status {
  attempting: bool,
  next_attempt_at: Option<chrono::DateTime<Utc>>,
}

impl Watchdog {
  /// A watchdog that is off, for `client`, connecting with the current `config` once `shares`
  /// are initialized. Its timer stops with `shutdown`.
  pub fn new(
    client: Client,
    config: Watcher<Options>,
    state: StateOwner,
    shares: Arc<Shares>,
    shutdown: CancellationToken,
  ) -> Self {
    let watchdog = Self {
      inner: Arc::new(Inner {
        client,
        config,
        state,
        shares,
        shutdown,
        enabled: watch::Sender::new(false),
        running: Arc::new(tokio::sync::Mutex::new(())),
        attempt: Mutex::new(None),
        failures: AtomicU32::new(0),
        unanswered: AtomicU32::new(0),
        status: Mutex::new(Status::default()),
      }),
    };
    tokio::spawn(
      watchdog
        .clone()
        .backstop()
        .instrument(info_span!("connection_watchdog")),
    );
    watchdog
  }

  /// Whether the watchdog is on.
  pub fn is_enabled(&self) -> bool {
    *self.inner.enabled.borrow()
  }

  /// Turns the watchdog on and starts a loop at once, unless one runs.
  pub fn start(&self) {
    self.inner.enabled.send_replace(true);
    self.publish();
    self.attempt_connection();
  }

  /// Turns the watchdog off, so no new loop starts and the running one ends after its attempt.
  /// With `abort`, its wait or attempt is canceled too, which a connect in progress reports as
  /// canceled.
  pub fn stop(&self, abort: bool) {
    self.inner.enabled.send_replace(false);
    self.publish();
    if abort && let Some(attempt) = lock(&self.inner.attempt).as_ref() {
      attempt.cancel();
    }
  }

  /// Starts over at once: the running loop and its back-off are canceled, the waits after
  /// unanswered logins forgotten, and a new loop starts as soon as the old one has ended.
  /// Returns whether the watchdog was on.
  pub fn restart(&self) -> bool {
    let was = self.is_enabled();
    if was {
      info!("(Re)connection process restarted");
    }
    self.stop(true);
    self.forget_failures();
    self.start();
    was
  }

  /// Forgets the failed attempts and the unanswered logins, once a login succeeded.
  pub fn logged_in(&self) {
    self.forget_failures();
    self.publish();
  }

  fn forget_failures(&self) {
    self.inner.failures.store(0, Ordering::Relaxed);
    self.inner.unanswered.store(0, Ordering::Relaxed);
  }

  /// Starts a loop unless one runs.
  fn attempt_connection(&self) {
    let Ok(running) = self.inner.running.clone().try_lock_owned() else {
      return;
    };
    let watchdog = self.clone();
    tokio::spawn(
      async move {
        watchdog.run().await;
        *lock(&watchdog.inner.status) = Status::default();
        *lock(&watchdog.inner.attempt) = None;
        watchdog.publish();
        drop(running);
      }
      .in_current_span(),
    );
  }

  /// Starts a loop every [`BACKSTOP`] while the watchdog is on and none runs.
  async fn backstop(self) {
    let mut enabled = self.inner.enabled.subscribe();
    loop {
      tokio::select! {
        () = self.inner.shutdown.cancelled() => return,
        on = enabled.wait_for(|on| *on) => if on.is_err() { return },
      }
      let mut every = tokio::time::interval(BACKSTOP);
      every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
      while self.is_enabled() {
        tokio::select! {
          () = self.inner.shutdown.cancelled() => return,
          _ = every.tick() => self.attempt_connection(),
        }
      }
    }
  }

  /// The connect loop: tries until connected, off, or canceled.
  async fn run(&self) {
    let inner = &self.inner;
    lock(&inner.status).attempting = true;
    while self.is_enabled() {
      let failed = inner.failures.load(Ordering::Relaxed);
      // connected already, as when asked to connect while connected
      if inner.client.state().is_connected() {
        self.stop(false);
        return;
      }
      let attempt = inner.shutdown.child_token();
      *lock(&inner.attempt) = Some(attempt.clone());
      let unanswered = inner.unanswered.load(Ordering::Relaxed);
      let wait = match unanswered_wait(unanswered) {
        Some(wait) => {
          error!(
            minutes = wait.as_secs() / 60,
            "the server did not answer the login; the account may be banned. Trying again later"
          );
          Some(wait)
        }
        None if failed > 0 => {
          let wait = backoff::wait(failed, backoff::AT_ONCE, BACKOFF_MAX);
          info!(
            seconds = wait.as_millis().div_ceil(1000),
            "Waiting before attempting to reconnect"
          );
          Some(wait)
        }
        None => None,
      };
      let start_at = Utc::now() + wait.unwrap_or_default();
      lock(&inner.status).next_attempt_at = Some(start_at);
      self.publish();
      if let Some(wait) = wait {
        tokio::select! {
          () = attempt.cancelled() => {
            debug!("Reconnection attempt canceled");
            return;
          }
          () = tokio::time::sleep(wait) => {}
        }
      }
      if !self.is_enabled() {
        return;
      }
      let span = info_span!("server_connect", attempt = failed + 1);
      match self.attempt(&attempt).instrument(span).await {
        Attempt::Connected | Attempt::Canceled => return,
        Attempt::Stop => {
          self.stop(false);
          return;
        }
        Attempt::Unanswered => {
          inner.unanswered.fetch_add(1, Ordering::Relaxed);
          inner.failures.fetch_add(1, Ordering::Relaxed);
        }
        Attempt::Failed => {
          inner.failures.fetch_add(1, Ordering::Relaxed);
        }
      }
    }
  }

  /// One attempt with the current options, once the shares are initialized.
  async fn attempt(&self, cancel: &CancellationToken) -> Attempt {
    let inner = &self.inner;
    info!("Attempting to connect to the Soulseek server");
    tokio::select! {
      () = cancel.cancelled() => {
        debug!("Reconnection attempt canceled");
        return Attempt::Canceled;
      }
      () = inner.shares.initialized() => {}
    }
    let options = inner.config.current();
    let soulseek = &options.values.soulseek;
    let Some((username, password)) = super::credentials(soulseek) else {
      super::warn_no_credentials();
      return Attempt::Stop;
    };
    match inner
      .client
      .connect(&soulseek.address, soulseek.port, username, password, cancel)
      .await
    {
      Ok(()) => Attempt::Connected,
      Err(ClientError::Canceled) => {
        debug!("Reconnection attempt canceled");
        Attempt::Canceled
      }
      Err(ClientError::LoginNoResponse(message)) => {
        error!(
          error = message,
          "Failed to reconnect: the server did not answer the login"
        );
        Attempt::Unanswered
      }
      // the disconnect it caused says why; nothing retries a refused login until asked to
      Err(ClientError::LoginRejected(reason)) => {
        debug!(reason, "login refused; not trying again");
        Attempt::Stop
      }
      Err(error @ ClientError::InvalidArgument(_)) => {
        error!(%error, "Not connecting to the Soulseek server; the options cannot log in");
        Attempt::Stop
      }
      Err(error) => {
        warn!(%error, "Failed to reconnect");
        Attempt::Failed
      }
    }
  }

  /// Mirrors the watchdog into `State.connectionWatchdog`.
  fn publish(&self) {
    let inner = &self.inner;
    let status = *lock(&inner.status);
    let watchdog = ConnectionWatchdogState {
      is_enabled: self.is_enabled(),
      is_attempting_connection: status.attempting,
      next_attempt_at: status
        .attempting
        .then_some(status.next_attempt_at)
        .flatten(),
      is_login_unanswered: inner.unanswered.load(Ordering::Relaxed) > 0,
    };
    inner
      .state
      .update(|state| state.connection_watchdog = watchdog);
  }
}

/// How one attempt ended.
enum Attempt {
  /// connected and logged in
  Connected,
  /// stopped by a restart, a disconnect asked for or shutdown
  Canceled,
  /// nothing the loop can retry: no credentials, or credentials the server or the client
  /// refuses
  Stop,
  /// the login was written and never answered
  Unanswered,
  /// anything else, retried after the back-off
  Failed,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn unanswered_logins_back_off_hard() {
    assert_eq!(unanswered_wait(0), None);
    assert_eq!(unanswered_wait(1), Some(Duration::from_secs(900)));
    assert_eq!(unanswered_wait(2), Some(Duration::from_secs(3600)));
    assert_eq!(unanswered_wait(3), Some(Duration::from_secs(21_600)));
    assert_eq!(unanswered_wait(50), Some(Duration::from_secs(21_600)));
  }
}
