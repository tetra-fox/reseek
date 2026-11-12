//! The agent's connection to its controller, one for the life of the program: it dials, logs
//! in, uploads its shares and serves the controller's requests; whenever the socket is lost, it
//! dials again, backing off up to 30 seconds and starting over once it was ready. A refusal of
//! its configuration, its API key or its shares, or another agent logging in under its instance
//! name, backs off to the cap at once and is warned about once per reason. A change to the
//! controller's settings closes the socket and dials again with the new ones.

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
  /// the last configuration problem warned about, so a repeated one is not
  warned: Mutex<Option<String>>,
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
    let agent = Arc::new_cyclic(|this| Self {
      this: this.clone(),
      config,
      shares,
      downloads,
      uploads,
      shutdown,
      link: Mutex::new(None),
      syncing: AtomicBool::new(false),
      again: AtomicBool::new(false),
      warned: Mutex::new(None),
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
      let ended = self.serve(socket, settings).await;
      shut_down = ended.code == Some(close::SHUTTING_DOWN);
      // two agents with one instance name would otherwise take the socket from each other
      // within a second, each time uploading their whole share index
      let superseded = ended.code == Some(close::SUPERSEDED);
      refused = superseded || ended.refusal.is_some();
      if ended.ready && !refused {
        failures = 1;
      }
      match &ended.refusal {
        Some(reason) => self.warn_once(reason, "the relay controller refused this agent"),
        None if superseded => {
          let current = self.config.current();
          let name = &current.values.instance_name;
          let reason = format!("another agent logged in as {name}");
          self.warn_once(
            &reason,
            "the relay controller dropped this agent for another",
          );
        }
        None => info!(code = ended.code, "relay controller connection lost"),
      }
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
