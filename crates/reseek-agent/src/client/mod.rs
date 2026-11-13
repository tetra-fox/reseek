//! The agent's connection to its controller, one for the life of the program: it dials, logs
//! in, uploads its shares and serves the controller's requests; whenever the socket is lost, it
//! dials again, backing off up to 30 seconds and starting over once it was ready. A refusal of
//! its configuration, its API key or its shares, or another agent logging in under its instance
//! name, backs off to the cap at once and is warned about once per reason; so is a controller it
//! cannot reach, until a socket is ready again. A change to the controller's settings closes the
//! socket and dials again with the new ones.

pub(crate) mod dial;
mod downloads;
mod files;
mod session;
mod sync;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use cascade::Watcher;
use reseek_relay_proto::frames::close;
use reseek_shares::{Shares, backoff, lock};
use tokio::sync::Notify;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, info_span, warn};

use crate::options::{Controller, Options};
use dial::{DialError, Settings, without_userinfo};
use session::{Ended, Link};

/// The longest wait between two attempts to reach the controller.
const BACKOFF_CAP: Duration = Duration::from_secs(30);

/// How long an agent waits at least after its controller closed for shutting down.
const AFTER_SHUTDOWN: Duration = Duration::from_secs(1);

/// The agent's connection to its controller.
pub struct AgentClient {
  this: Weak<AgentClient>,
  config: Watcher<Options>,
  /// the instance name the agent started with, which only a restart changes
  name: String,
  shares: Arc<Shares>,
  /// the downloads directory, which the controller's finished downloads are copied to
  downloads: PathBuf,
  /// where the share index is copied to before it is uploaded
  uploads: PathBuf,
  shutdown: CancellationToken,
  /// the socket now open
  link: Mutex<Option<Arc<Link>>>,
  /// a share upload runs
  syncing: AtomicBool,
  /// another share upload was asked for while one ran
  again: AtomicBool,
  /// the last refusal or configuration problem warned about, so a repeated one is not; forgotten
  /// when the controller's settings change or a socket that was ready ends
  warned: Mutex<Option<String>>,
  /// why the last failed dial was warned about, so the same failure again is not; forgotten when
  /// a dial succeeds and with `warned`
  dial_warned: Mutex<Option<String>>,
  /// the last share upload failure warned about, so a repeated one is not; forgotten once an
  /// upload succeeds
  sync_warned: Mutex<Option<String>>,
  /// the controller's settings changed, so the next dial is at once
  redial: Notify,
}

impl AgentClient {
  /// The agent of the options in `config`, serving `shares` and copying finished downloads
  /// into `downloads`, until `shutdown`; the share index is copied into `uploads` to be sent.
  /// It follows the options and the shares at once and connects once [`AgentClient::run`]
  /// runs.
  pub fn new(
    config: Watcher<Options>,
    shares: Arc<Shares>,
    downloads: PathBuf,
    uploads: PathBuf,
    shutdown: CancellationToken,
  ) -> Arc<Self> {
    let name = config.current().values.instance_name.clone();
    let agent = Arc::new_cyclic(|this| Self {
      this: this.clone(),
      config,
      name,
      shares,
      downloads,
      uploads,
      shutdown,
      link: Mutex::new(None),
      syncing: AtomicBool::new(false),
      again: AtomicBool::new(false),
      warned: Mutex::new(None),
      dial_warned: Mutex::new(None),
      sync_warned: Mutex::new(None),
      redial: Notify::new(),
    });
    let span = info_span!("relay_agent");
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

  /// The socket now open.
  fn current(&self) -> Option<Arc<Link>> {
    lock(&self.link).clone()
  }

  /// Dials, serves the socket and dials again until the program stops; returns once the last
  /// socket is closed.
  pub async fn run(self: Arc<Self>) {
    let mut failures: u32 = 0;
    let mut refused = false;
    let mut shut_down = false;
    loop {
      if self.shutdown.is_cancelled() {
        return;
      }
      if failures > 0 {
        let mut delay = if refused {
          backoff::wait(u32::MAX, backoff::AT_ONCE, BACKOFF_CAP)
        } else {
          backoff::wait(failures, backoff::AT_ONCE, BACKOFF_CAP)
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
      }
      failures += 1;
      let current = self.config.current();
      let settings = match Settings::new(&current.values, &self.name).await {
        Ok(settings) => Arc::new(settings),
        Err(e) => {
          refused = true;
          warn_once(&self.warned, &e, "relay controller settings not usable");
          continue;
        }
      };
      let socket = tokio::select! {
        () = self.shutdown.cancelled() => return,
        socket = settings.dial() => socket,
      };
      let socket = match socket {
        Ok(socket) => socket,
        Err(DialError::Refused(reason)) => {
          refused = true;
          warn_once(
            &self.warned,
            &reason,
            "the relay controller refused this agent",
          );
          continue;
        }
        Err(DialError::Failed(e)) => {
          refused = false;
          self.dial_failed(&e);
          continue;
        }
      };
      info!(address = %without_userinfo(&settings.address), "connected to the relay controller");
      *lock(&self.dial_warned) = None;
      let ended = self.serve(socket, settings).await;
      shut_down = ended.code == Some(close::SHUTTING_DOWN);
      // two agents with one instance name would otherwise take the socket from each other
      // within a second, each time uploading their whole share index
      let superseded = ended.code == Some(close::SUPERSEDED);
      refused = superseded || ended.refusal.is_some();
      if ended.ready && !refused {
        failures = 1;
      }
      self.report_end(&ended);
    }
  }

  /// Logs a dial that failed with `error`, warned about once until it fails another way or a
  /// dial succeeds.
  fn dial_failed(&self, error: &str) {
    warn_once(
      &self.dial_warned,
      error,
      "Failed attempt to connect to relay controller",
    );
  }

  /// Forgets the refusals and the failed dial warned about, so the next is warned about again.
  fn forget_warnings(&self) {
    *lock(&self.warned) = None;
    *lock(&self.dial_warned) = None;
  }

  /// Logs how a socket `ended`: a refusal or another agent taking it is warned about once per
  /// reason, anything else told at info. A socket that was ready ends an outage, so what
  /// follows it is warned about again.
  fn report_end(&self, ended: &Ended) {
    match &ended.refusal {
      Some(reason) => warn_once(
        &self.warned,
        reason,
        "the relay controller refused this agent",
      ),
      None if ended.code == Some(close::SUPERSEDED) => {
        let reason = format!("another agent logged in as {}", self.name);
        warn_once(
          &self.warned,
          &reason,
          "the relay controller dropped this agent for another",
        );
      }
      None => {
        if ended.ready {
          self.forget_warnings();
        }
        info!(code = ended.code, "relay controller connection lost");
      }
    }
  }

  /// Closes the socket when the controller's settings change, so the next dial uses the new
  /// ones; whether finished downloads are fetched is read when one is offered.
  async fn follow_configuration(self: Arc<Self>) {
    let fingerprint = |options: &Options| {
      let Controller {
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
      self.forget_warnings();
      *lock(&self.sync_warned) = None;
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

/// Logs `reason` as a warning unless it is the last one `warned` holds; then at debug.
fn warn_once(warned: &Mutex<Option<String>>, reason: &str, message: &'static str) {
  let mut warned = lock(warned);
  if warned.as_deref() == Some(reason) {
    debug!(reason, message);
  } else {
    warn!(reason, message);
    *warned = Some(reason.to_string());
  }
}
