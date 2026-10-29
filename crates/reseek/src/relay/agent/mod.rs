//! The agent: one connection to its controller for the life of the daemon. Once started it
//! dials, logs in, uploads its shares and serves the controller's requests; whenever the socket
//! is lost while it is still meant to be connected, it dials again, backing off up to 30
//! seconds and starting over once it was ready. A refusal of its configuration backs off to the
//! cap at once and is warned about once per reason. A change to the controller's settings
//! closes the socket and dials again with the new ones.

mod dial;
mod downloads;
mod files;
mod session;
mod sync;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use api_types::relay::RelayConnectionState;
use cascade::Watcher;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, info_span, warn};

use super::frames::close;
use crate::backoff;
use crate::config::{Options, RelayController};
use crate::shares::Shares;
use crate::state::StateOwner;
use crate::sync::lock;
use dial::Settings;
use session::Link;

/// The longest wait between two attempts to reach the controller.
const BACKOFF_CAP: Duration = Duration::from_secs(30);

/// How long an agent waits at least after its controller closed for shutting down.
const AFTER_SHUTDOWN: Duration = Duration::from_secs(1);

/// The agent's connection to its controller.
pub struct AgentClient {
  this: Weak<AgentClient>,
  config: Watcher<Options>,
  shares: Arc<Shares>,
  state: StateOwner,
  /// the downloads directory, which the controller's finished downloads are copied to
  downloads: PathBuf,
  /// where the share index is written before it is uploaded
  temporary: PathBuf,
  /// this daemon is its own controller, so copies of its downloads are named apart
  debug: bool,
  shutdown: CancellationToken,
  /// whether the agent is meant to be connected
  started: watch::Sender<bool>,
  /// the socket now open
  link: Mutex<Option<Arc<Link>>>,
  /// a share upload runs
  syncing: AtomicBool,
  /// another share upload was asked for while one ran
  again: AtomicBool,
  /// the last configuration problem warned about, so a repeated one is not
  warned: Mutex<Option<String>>,
  /// the controller's settings changed, so the next dial is at once
  redial: Notify,
}

impl AgentClient {
  /// The agent of the options, copying finished downloads into `downloads`; `debug` when
  /// the daemon is also the controller. Nothing is dialed until [`AgentClient::start`].
  pub fn new(
    config: Watcher<Options>,
    shares: Arc<Shares>,
    state: StateOwner,
    downloads: PathBuf,
    debug: bool,
    shutdown: CancellationToken,
  ) -> Arc<Self> {
    let agent = Arc::new_cyclic(|this| Self {
      this: this.clone(),
      config,
      shares,
      state,
      downloads,
      temporary: std::env::temp_dir().join("reseek"),
      debug,
      shutdown,
      started: watch::Sender::new(false),
      link: Mutex::new(None),
      syncing: AtomicBool::new(false),
      again: AtomicBool::new(false),
      warned: Mutex::new(None),
      redial: Notify::new(),
    });
    let span = info_span!("relay_agent");
    tokio::spawn(agent.clone().run().instrument(span.clone()));
    tokio::spawn(
      agent
        .clone()
        .follow_configuration()
        .instrument(span.clone()),
    );
    tokio::spawn(agent.clone().follow_shares().instrument(span));
    agent
  }

  fn this(&self) -> Arc<Self> {
    self
      .this
      .upgrade()
      .expect("the agent is only used inside the Arc it refers to")
  }

  /// Starts connecting to the controller; nothing when it is started already.
  pub fn start(&self) {
    if !self.started.send_replace(true) {
      info!("relay agent connection started");
    }
  }

  /// Closes the connection to the controller and stops dialing.
  pub fn stop(&self) {
    self.started.send_replace(false);
    if let Some(link) = self.current() {
      link.close(close::NORMAL);
    }
    self.set_state(RelayConnectionState::Disconnected);
  }

  /// Uploads the shares again now; false when the agent is not logged in.
  pub fn sync_now(&self) -> bool {
    if !self.current().is_some_and(|link| link.logged_in()) {
      return false;
    }
    self.request_sync();
    true
  }

  /// The socket now open.
  fn current(&self) -> Option<Arc<Link>> {
    lock(&self.link).clone()
  }

  fn set_state(&self, state: RelayConnectionState) {
    self
      .state
      .update(|current| current.relay.controller.state = state);
  }

  /// Waits until the agent is meant to be connected; false when the daemon stops first.
  async fn started(&self) -> bool {
    let mut started = self.started.subscribe();
    tokio::select! {
      () = self.shutdown.cancelled() => false,
      waited = started.wait_for(|started| *started) => waited.is_ok(),
    }
  }

  /// Dials, serves the socket and dials again while the agent is meant to be connected.
  async fn run(self: Arc<Self>) {
    let mut failures: u32 = 0;
    let mut refused = false;
    let mut shut_down = false;
    loop {
      if !self.started().await {
        return;
      }
      if failures > 0 {
        let mut delay = if refused {
          backoff::wait(u32::MAX, BACKOFF_CAP)
        } else {
          backoff::wait(failures, BACKOFF_CAP)
        };
        if shut_down {
          delay = delay.max(AFTER_SHUTDOWN);
        }
        debug!(
          attempt = failures + 1,
          ?delay,
          "dialing the relay controller again after a wait"
        );
        tokio::select! {
          () = self.shutdown.cancelled() => return,
          () = tokio::time::sleep(delay) => {}
          () = self.redial.notified() => failures = 0,
        }
        if !*self.started.borrow() {
          continue;
        }
      }
      self.set_state(if failures == 0 {
        RelayConnectionState::Connecting
      } else {
        RelayConnectionState::Reconnecting
      });
      let attempt = failures + 1;
      failures += 1;
      let settings = match Settings::new(&self.config.current().values) {
        Ok(settings) => Arc::new(settings),
        Err(e) => {
          refused = true;
          self.warn_once(&e, "relay controller settings not usable");
          continue;
        }
      };
      let socket = tokio::select! {
        () = self.shutdown.cancelled() => return,
        socket = settings.dial() => socket,
      };
      let socket = match socket {
        Ok(socket) => socket,
        Err(e) => {
          refused = false;
          warn!(
            attempt,
            error = e,
            "Failed attempt to connect to relay controller"
          );
          continue;
        }
      };
      info!(address = %settings.address, "connected to the relay controller");
      self.set_state(RelayConnectionState::Authenticating);
      let ended = self.serve(socket, settings).await;
      if ended.ready {
        failures = 1;
      }
      shut_down = ended.code == Some(close::SHUTTING_DOWN);
      refused = ended.refusal.is_some();
      match &ended.refusal {
        Some(reason) => self.warn_once(reason, "the relay controller refused this agent"),
        None => info!(code = ended.code, "relay controller connection lost"),
      }
      self.set_state(if *self.started.borrow() && !self.shutdown.is_cancelled() {
        RelayConnectionState::Reconnecting
      } else {
        RelayConnectionState::Disconnected
      });
    }
  }

  /// Logs `reason` as a warning unless it was the last one warned about; then at debug.
  fn warn_once(&self, reason: &str, message: &'static str) {
    let mut warned = lock(&self.warned);
    if warned.as_deref() == Some(reason) {
      debug!(reason, message);
    } else {
      warn!(reason, message);
      *warned = Some(reason.to_string());
    }
  }

  /// Closes the socket when the controller's settings change, so the next dial uses the new
  /// ones; whether finished downloads are fetched is read when one is offered.
  async fn follow_configuration(self: Arc<Self>) {
    let fingerprint = |options: &Options| {
      let RelayController {
        address,
        ignore_certificate_errors,
        api_key,
        secret,
        ..
      } = options.relay.controller.clone();
      (address, ignore_certificate_errors, api_key, secret)
    };
    let mut changes = self.config.changes();
    let mut applied = fingerprint(&self.config.current().values);
    loop {
      let change = tokio::select! {
        () = self.shutdown.cancelled() => return,
        change = changes.recv() => change,
      };
      if let Err(RecvError::Closed) = change {
        return;
      }
      let now = fingerprint(&self.config.current().values);
      if now == applied {
        continue;
      }
      applied = now;
      info!("relay controller settings changed; connecting again with them");
      *lock(&self.warned) = None;
      if let Some(link) = self.current() {
        link.close(close::NORMAL);
      }
      self.redial.notify_one();
    }
  }

  /// Uploads the shares again whenever the local index changed.
  async fn follow_shares(self: Arc<Self>) {
    let mut changes = self.shares.changes();
    loop {
      tokio::select! {
        () = self.shutdown.cancelled() => return,
        changed = changes.changed() => if changed.is_err() { return },
      }
      debug!("local shares changed");
      self.request_sync();
    }
  }
}
