//! Keeping the server connection up. The watchdog runs from the start and after a connection
//! is lost, and stops once the client is connected, when asked to disconnect, and at shutdown;
//! a refused login does not start it again, and once a disconnect was asked for, or at
//! shutdown, a lost connection does not either: only a start does. One connect loop runs at a
//! time: the first attempt at once, then each after an exponential back-off with jitter, or,
//! after a login the server never answered, 15 minutes, an hour and then 6 hours apart. A timer
//! starts a new loop while the watchdog is on and none runs. The back-off goes on across loops
//! until a login succeeds or the watchdog is restarted, so a server that drops us during every
//! login is not tried again at once each time.

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
  /// whether the watchdog is on, and what may turn it on
  mode: watch::Sender<Mode>,
  /// held by the loop that runs
  running: Arc<tokio::sync::Mutex<()>>,
  /// the running turn and the counts its wait is read from
  turns: Mutex<Turns>,
  /// whether a loop runs and when its next attempt starts
  status: Mutex<Status>,
}

/// Whether the watchdog is on, and what may turn it on
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
  /// the timer runs and the loop keeps trying
  On,
  /// off while connected or after a login it cannot retry; a lost connection turns it on
  Off,
  /// off since a disconnect was asked for or the daemon began to stop; only a start turns it on
  Held,
}

/// The running turn's token and the failures counted so far, under one lock, so a restart's
/// clear and cancel fall wholly before or after a turn's read of the counts and its count
#[derive(Default)]
struct Turns {
  /// cancels the running turn's wait or attempt
  token: Option<CancellationToken>,
  /// attempts that failed since the last login or restart
  failures: u32,
  /// logins in a row the server left unanswered
  unanswered: u32,
}

impl Turns {
  /// Stores a new turn's token and returns it with the failures and unanswered logins its
  /// wait is read from
  fn begin(&mut self, token: CancellationToken) -> (CancellationToken, u32, u32) {
    self.token = Some(token.clone());
    (token, self.failures, self.unanswered)
  }

  /// Counts the failed attempt of the turn `token` belongs to, an unanswered login when
  /// `unanswered`. A turn whose token was canceled counts nothing: a restart canceled it and
  /// cleared the counts, which its failure must not bring back.
  fn count(&mut self, token: &CancellationToken, unanswered: bool) -> bool {
    if token.is_cancelled() {
      return false;
    }
    self.failures += 1;
    if unanswered {
      self.unanswered += 1;
    }
    true
  }

  fn clear(&mut self) {
    self.failures = 0;
    self.unanswered = 0;
  }

  fn cancel(&self) {
    if let Some(token) = &self.token {
      token.cancel();
    }
  }
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
        mode: watch::Sender::new(Mode::Off),
        running: Arc::new(tokio::sync::Mutex::new(())),
        turns: Mutex::new(Turns::default()),
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
    *self.inner.mode.borrow() == Mode::On
  }

  /// Turns the watchdog on and starts a loop at once, unless one runs.
  pub fn start(&self) {
    self.inner.mode.send_replace(Mode::On);
    self.publish();
    self.attempt_connection();
  }

  /// A session that reached the server was lost: the watchdog is turned on as by a start,
  /// unless it was held off since.
  pub fn lost(&self) {
    let mut held = false;
    self.inner.mode.send_if_modified(|mode| match mode {
      Mode::Held => {
        held = true;
        false
      }
      Mode::On => false,
      Mode::Off => {
        *mode = Mode::On;
        true
      }
    });
    if held {
      return debug!("a disconnect was asked for; the lost connection is not reconnected");
    }
    self.publish();
    self.attempt_connection();
  }

  /// Turns the watchdog off, so no new loop starts and the running one ends after its attempt;
  /// a watchdog held off stays held. With `abort`, its wait or attempt is canceled too, which a
  /// connect in progress reports as canceled.
  pub fn stop(&self, abort: bool) {
    self.inner.mode.send_if_modified(|mode| {
      let on = *mode == Mode::On;
      if on {
        *mode = Mode::Off;
      }
      on
    });
    self.publish();
    if abort {
      self.cancel_attempt();
    }
  }

  /// Turns the watchdog off until it is started again, canceling its wait or attempt: a
  /// connection lost before or after does not turn it on.
  pub fn hold(&self) {
    self.inner.mode.send_replace(Mode::Held);
    self.publish();
    self.cancel_attempt();
  }

  fn cancel_attempt(&self) {
    lock(&self.inner.turns).cancel();
  }

  /// Starts over at once: the running loop and its back-off are canceled, the waits after
  /// unanswered logins forgotten, and a new loop starts as soon as the old one has ended.
  /// Returns whether the watchdog was on.
  pub fn restart(&self) -> bool {
    let was = self.is_enabled();
    if was {
      info!("(Re)connection process restarted");
    }
    // cleared and canceled under the lock a turn reads and counts under: a turn that began
    // before is canceled and counts nothing, one that begins after reads the cleared counts
    {
      let mut turns = lock(&self.inner.turns);
      turns.clear();
      turns.cancel();
    }
    self.stop(false);
    self.start();
    was
  }

  /// Forgets the failed attempts and the unanswered logins, once a login succeeded.
  pub fn logged_in(&self) {
    lock(&self.inner.turns).clear();
    self.publish();
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
        lock(&watchdog.inner.turns).token = None;
        watchdog.publish();
        drop(running);
      }
      .in_current_span(),
    );
  }

  /// Starts a loop every [`BACKSTOP`] while the watchdog is on and none runs.
  async fn backstop(self) {
    let mut mode = self.inner.mode.subscribe();
    loop {
      tokio::select! {
        () = self.inner.shutdown.cancelled() => return,
        on = mode.wait_for(|mode| *mode == Mode::On) => if on.is_err() { return },
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
      // connected already, as when asked to connect while connected
      if inner.client.state().is_connected() {
        self.stop(false);
        return;
      }
      // read with the token stored, under the lock a restart clears and cancels under: it
      // either cancels this turn or has already cleared what this reads
      let (attempt, failed, unanswered) = lock(&inner.turns).begin(inner.shutdown.child_token());
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
        ended @ (Attempt::Unanswered | Attempt::Failed) => {
          let unanswered = matches!(ended, Attempt::Unanswered);
          if !lock(&inner.turns).count(&attempt, unanswered) {
            debug!("failed attempt of a canceled turn not counted");
          }
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

  /// Mirrors the watchdog into `State.connectionWatchdog`. The watchdog is read inside the
  /// update, which takes one at a time, so the last of several publishing at once reads what
  /// every one of them changed before it.
  fn publish(&self) {
    let inner = &self.inner;
    inner.state.update(|state| {
      let status = *lock(&inner.status);
      state.connection_watchdog = ConnectionWatchdogState {
        is_enabled: self.is_enabled(),
        is_attempting_connection: status.attempting,
        next_attempt_at: status
          .attempting
          .then_some(status.next_attempt_at)
          .flatten(),
        is_login_unanswered: lock(&inner.turns).unanswered > 0,
      };
    });
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
  fn a_turn_canceled_by_a_restart_counts_nothing() {
    let mut turns = Turns::default();
    let (first, _, _) = turns.begin(CancellationToken::new());
    assert!(turns.count(&first, true));
    let (second, failed, unanswered) = turns.begin(CancellationToken::new());
    assert_eq!((failed, unanswered), (1, 1));
    // a restart clears and cancels before the second turn's unanswered login is counted
    turns.clear();
    turns.cancel();
    assert!(!turns.count(&second, true));
    let (_, failed, unanswered) = turns.begin(CancellationToken::new());
    assert_eq!((failed, unanswered), (0, 0), "the next turn starts at once");
  }

  #[test]
  fn unanswered_logins_back_off_hard() {
    assert_eq!(unanswered_wait(0), None);
    assert_eq!(unanswered_wait(1), Some(Duration::from_secs(900)));
    assert_eq!(unanswered_wait(2), Some(Duration::from_secs(3600)));
    assert_eq!(unanswered_wait(3), Some(Duration::from_secs(21_600)));
    assert_eq!(unanswered_wait(50), Some(Duration::from_secs(21_600)));
  }
}
