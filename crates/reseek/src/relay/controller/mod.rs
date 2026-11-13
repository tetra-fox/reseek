//! The controller: the agents logged in on their sockets, the tokens and waits of what it asked
//! them for, and what follows an agent's coming and going. An agent is registered under its
//! instance name once its login checks out and while its configuration still admits it; a
//! second login under the same name closes the first socket. Leaving, an agent takes its shares
//! with it, every wait on it fails at once, and what it was still sending stops.

mod downloads;
mod files;
mod info;
mod login;
mod session;
mod shares;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use api_types::events::{Event, RelayAgentConnected, RelayAgentDisconnected};
use api_types::relay::{LoginFailure, RelayAgentState, RelayFileUploadResult};
use cascade::Watcher;
use reseek_shares::{View, lock};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, debug, info, info_span, warn};
use uuid::Uuid;

use crate::api::error::ApiError;
use crate::config::cidr::parse_trimmed_list;
use crate::config::{Options, RelayAgent};
use crate::events::EventBus;
use crate::services::AgentCredential;
use crate::shares::Shares;
use crate::state::StateOwner;
use crate::telemetry::Telemetry;
use crate::telemetry::catalog::RelayMetrics;
pub use files::RelayStream;
use files::StreamWait;
use info::FileInfoWait;
use reseek_relay_proto::credential::{Key, Purpose};
use reseek_relay_proto::frames::close;

/// How many frames wait to be written to one agent's socket.
const OUTBOUND: usize = 64;

/// What the controller writes to an agent's socket.
#[derive(Debug)]
enum Outbound {
  /// a text frame
  Frame(String),
  /// the close frame, after which the socket ends
  Close(u16, &'static str),
}

/// An agent logged in on a socket.
struct Registration {
  /// the socket it logged in on
  socket: u64,
  /// what the state shows of it
  record: RelayAgentState,
  /// frames for its socket
  outbound: mpsc::Sender<Outbound>,
  /// the address it logged in from
  address: IpAddr,
  /// the secret it logged in with
  secret: String,
  /// the key derived from that secret, which its calls over HTTP are checked with
  key: Key,
  /// canceled when the registration ends, which stops the share upload and the files it is
  /// still sending
  left: CancellationToken,
}

/// A token handed to an agent on its socket to upload its shares with.
struct ShareToken {
  /// the agent it was handed to
  agent: String,
  /// the socket it was handed on
  socket: u64,
  expires: Instant,
}

/// A token handed to an agent to fetch a finished download with.
struct DownloadToken {
  /// the agent it was handed to
  agent: String,
  /// the download's path relative to the downloads directory
  filename: String,
  expires: Instant,
}

/// What an ended registration leaves behind: its waits, which fail, and what it was sending,
/// which stops.
struct Left {
  registration: Registration,
  infos: Vec<FileInfoWait>,
  streams: Vec<StreamWait>,
}

/// Everything the controller keeps about its agents, under one lock.
#[derive(Default)]
struct Registry {
  agents: BTreeMap<String, Registration>,
  share_tokens: HashMap<Uuid, ShareToken>,
  download_tokens: HashMap<Uuid, DownloadToken>,
  file_infos: HashMap<Uuid, FileInfoWait>,
  streams: HashMap<Uuid, StreamWait>,
  /// the answers of the agents' file uploads waiting for their transfer to end, by transfer
  completions: HashMap<Uuid, oneshot::Sender<RelayFileUploadResult>>,
  /// the agents whose share upload is being loaded
  syncing: HashSet<String>,
}

/// The relay controller.
pub struct Controller {
  config: Watcher<Options>,
  shares: Arc<Shares>,
  events: EventBus,
  state: StateOwner,
  /// counts the agents and what is asked of them
  telemetry: Arc<Telemetry>,
  /// the downloads directory, which finished downloads are offered from
  downloads: PathBuf,
  shutdown: CancellationToken,
  registry: Mutex<Registry>,
  /// the tasks serving the agents' sockets, which a stop waits for
  sessions: TaskTracker,
  sockets: AtomicU64,
}

impl Controller {
  /// The controller serving the agents of the options, loading their shares into `shares`,
  /// raising its events on `events`, keeping `State.relay.agents` current and counting in
  /// `telemetry`. It follows the agents' configuration until `shutdown`, which closes every
  /// agent's socket.
  pub fn new(
    config: Watcher<Options>,
    shares: Arc<Shares>,
    events: EventBus,
    state: StateOwner,
    telemetry: Arc<Telemetry>,
    downloads: PathBuf,
    shutdown: CancellationToken,
  ) -> Arc<Self> {
    let controller = Arc::new(Self {
      config,
      shares,
      events,
      state,
      telemetry,
      downloads,
      shutdown,
      registry: Mutex::new(Registry::default()),
      sessions: TaskTracker::new(),
      sockets: AtomicU64::new(0),
    });
    tokio::spawn(
      controller
        .clone()
        .follow_configuration()
        .instrument(info_span!("relay_controller")),
    );
    controller
  }

  /// The agents logged in, by name.
  pub fn agents(&self) -> Vec<RelayAgentState> {
    lock(&self.registry)
      .agents
      .values()
      .map(|r| r.record.clone())
      .collect()
  }

  /// Whether the agent `name` is logged in and its shares are loaded.
  pub fn ready(&self, name: &str) -> bool {
    lock(&self.registry)
      .agents
      .get(name)
      .is_some_and(|r| r.record.ready)
  }

  /// Whether `filename`, which no loaded share holds, was in the last index of an agent that is
  /// configured but is not serving its files now, away or loading them again, and `view` sees
  /// it. A peer asking for it can be told to ask again later.
  pub async fn away_holds(&self, filename: &str, view: &View) -> bool {
    let holding = self.shares.away_holding(filename, view).await;
    let options = self.config.current();
    let registry = lock(&self.registry);
    holding.iter().any(|name| {
      Self::configured(&options.values, name).is_some()
        && !registry.agents.get(name).is_some_and(|r| r.record.ready)
    })
  }

  /// Closes the socket of the agent `name` and unloads its shares; false when it is not logged
  /// in.
  pub fn kick(&self, name: &str) -> bool {
    let socket = lock(&self.registry).agents.get(name).map(|r| r.socket);
    let Some(socket) = socket else {
      return false;
    };
    self.leave(name, socket, close::KICKED, "kicked");
    true
  }

  /// The configured agent whose instance name is `name`.
  fn configured(options: &Options, name: &str) -> Option<RelayAgent> {
    options
      .relay
      .agents
      .values()
      .find(|agent| agent.instance_name.as_deref() == Some(name))
      .cloned()
  }

  /// Whether `address` is one the configured agent may connect from.
  fn address_allowed(agent: &RelayAgent, address: IpAddr) -> bool {
    // validated when the options were loaded, so a list that does not read allows nothing
    parse_trimmed_list(&agent.cidr).is_ok_and(|ranges| ranges.iter().any(|r| r.contains(address)))
  }

  /// Whether the agent `name`, logged in as `registration`, is still configured with the
  /// secret it logged in with and may still connect from the address it logged in from; why
  /// not, as its login would be refused.
  fn admits(
    options: &Options,
    name: &str,
    registration: &Registration,
  ) -> Result<(), LoginFailure> {
    let agent = Self::configured(options, name).ok_or(LoginFailure::UnknownAgent)?;
    if !Self::address_allowed(&agent, registration.address) {
      return Err(LoginFailure::AddressNotAllowed);
    }
    if agent.secret.as_deref() != Some(registration.secret.as_str()) {
      return Err(LoginFailure::BadCredential);
    }
    Ok(())
  }

  /// Checks a call an agent makes over HTTP with `token` for `purpose`: it is logged in,
  /// configured with the secret it logged in with, calling from one of its networks, and its
  /// credential is the one for the token. Each failure is a 401, logged.
  fn authenticate(
    &self,
    caller: &AgentCredential,
    purpose: Purpose,
    token: Uuid,
  ) -> Result<(), ApiError> {
    let name = caller.agent.as_str();
    let options = self.config.current();
    let configured = Self::configured(&options.values, name);
    let current_secret = configured.as_ref().and_then(|a| a.secret.as_deref());
    let logged_in = lock(&self.registry)
      .agents
      .get(name)
      .map(|r| (current_secret == Some(r.secret.as_str()), r.key.clone()));
    let Some((same_secret, key)) = logged_in else {
      warn!(
        agent = name,
        "relay call from an agent that is not logged in"
      );
      return Err(ApiError::unauthorized(format!("Agent {name} is not registered")).logged());
    };
    let Some(agent) = configured else {
      warn!(
        agent = name,
        "relay call from an agent that is not configured"
      );
      return Err(ApiError::unauthorized(format!("Agent {name} is not configured")).logged());
    };
    if !Self::address_allowed(&agent, caller.address) {
      warn!(
        agent = name,
        address = %caller.address,
        cidr = agent.cidr,
        "relay call from outside the agent's networks"
      );
      return Err(ApiError::unauthorized("The agent may not call from this address").logged());
    }
    if !same_secret {
      warn!(
        agent = name,
        "relay call from an agent whose secret changed since it logged in"
      );
      return Err(ApiError::unauthorized("The agent's secret changed since it logged in").logged());
    }
    if !key.verifies(purpose, &token.to_string(), &caller.credential) {
      warn!(agent = name, %token, "relay call with a credential mismatch");
      return Err(ApiError::unauthorized("The credential does not match").logged());
    }
    debug!(agent = name, %token, "Agent authenticated for token");
    Ok(())
  }

  /// Registers the agent of `registration` in place of any earlier socket of the same name,
  /// which is closed, and raises `RelayAgentConnected`; refused when the configuration no
  /// longer admits it, as when it changed while the login was checked.
  fn register(&self, registration: Registration) -> Result<(), LoginFailure> {
    let name = registration.record.name.clone();
    let ip_address = registration.record.ip_address.clone();
    let earlier = {
      let mut registry = lock(&self.registry);
      // read under the lock, so a configuration change after this finds the registration when
      // it looks for agents it no longer admits
      let options = self.config.current();
      Self::admits(&options.values, &name, &registration)?;
      let earlier = registry
        .agents
        .get(&name)
        .map(|r| r.socket)
        .and_then(|socket| take(&mut registry, &name, socket));
      registry.agents.insert(name.clone(), registration);
      earlier
    };
    if let Some(earlier) = earlier {
      self.closed_on_leaving(&name, earlier, close::SUPERSEDED, "superseded");
    }
    self.publish();
    self
      .events
      .raise(Event::RelayAgentConnected(RelayAgentConnected {
        version: 0,
        agent: name,
        ip_address,
      }));
    Ok(())
  }

  /// Ends the registration of the agent `name` on `socket`, closing it with `code`, when that
  /// socket is the one registered.
  fn leave(&self, name: &str, socket: u64, code: u16, reason: &'static str) {
    let left = take(&mut lock(&self.registry), name, socket);
    if let Some(left) = left {
      self.closed_on_leaving(name, left, code, reason);
    }
  }

  /// Asks the socket of the registration in `left` to close with `code`, then does what
  /// follows its leaving.
  fn closed_on_leaving(&self, name: &str, left: Left, code: u16, reason: &'static str) {
    // a socket whose queue is full or closed is ending on its own
    if let Err(e) = left
      .registration
      .outbound
      .try_send(Outbound::Close(code, reason))
    {
      debug!(agent = name, error = %e, "close not queued for the agent's socket");
    }
    self.after_leaving(name, left, reason);
  }

  /// Removes the agent `name` when it is registered on `socket` and not on a newer one, and
  /// does what follows its leaving.
  fn deregister(&self, name: &str, socket: u64, reason: &str) {
    let left = take(&mut lock(&self.registry), name, socket);
    if let Some(left) = left {
      self.after_leaving(name, left, reason);
    }
  }

  /// What follows the agent `name` leaving: what it was sending stops, every wait on it fails,
  /// its shares are unloaded, and `RelayAgentDisconnected` is raised.
  fn after_leaving(&self, name: &str, left: Left, reason: &str) {
    let Left {
      registration,
      infos,
      streams,
    } = left;
    registration.left.cancel();
    let pending = infos.len() + streams.len();
    // the waits learn their agent left by their sender going
    drop(infos);
    for wait in streams {
      wait.fail(files::StreamFailure::Disconnected);
    }
    info!(agent = name, reason, pending, "Agent disconnected");
    self.shares.remove_host(name);
    self.publish();
    self
      .events
      .raise(Event::RelayAgentDisconnected(RelayAgentDisconnected {
        version: 0,
        agent: name.to_string(),
        reason: reason.to_string(),
      }));
  }

  /// Puts the agents into `State.relay.agents`.
  fn publish(&self) {
    let agents = self.agents();
    self
      .relay_metrics()
      .agents_connected
      .set(agents.len() as f64);
    self.state.update(|state| state.relay.agents = agents);
  }

  fn relay_metrics(&self) -> &RelayMetrics {
    &self.telemetry.metrics().relay
  }

  /// Counts a request of `kind` that ended with `outcome`.
  fn count(&self, kind: &str, outcome: &str) {
    self
      .relay_metrics()
      .requests
      .inc(&[("type", kind), ("outcome", outcome)]);
  }

  /// Queues `frame` for the agent `name`'s socket; false when it is not logged in or its queue
  /// is full.
  fn send(&self, name: &str, frame: String) -> bool {
    let outbound = lock(&self.registry)
      .agents
      .get(name)
      .map(|r| r.outbound.clone());
    let Some(outbound) = outbound else {
      return false;
    };
    match outbound.try_send(Outbound::Frame(frame)) {
      Ok(()) => true,
      Err(e) => {
        warn!(agent = name, error = %e, "frame for the agent's socket not queued");
        false
      }
    }
  }

  /// Disconnects the agents whose configuration was removed, whose instance name or secret
  /// changed, or whose networks no longer hold the address they logged in from, until the
  /// daemon stops.
  async fn follow_configuration(self: Arc<Self>) {
    let mut changes = self.config.changes();
    // the indexes an earlier run kept include those of agents configured no more
    self.drop_unconfigured();
    loop {
      let change = tokio::select! {
        () = self.shutdown.cancelled() => return,
        change = changes.recv() => change,
      };
      match change {
        Ok(_) | Err(RecvError::Lagged(_)) => self.drop_unconfigured(),
        Err(RecvError::Closed) => return,
      }
    }
  }

  /// Waits, once the daemon is stopping, until every agent's socket was closed as shutting down,
  /// for at most `wait`; the HTTP host stops accepting after this, so agents learn of the stop
  /// first.
  pub async fn closed(&self, wait: std::time::Duration) {
    self.sessions.close();
    if tokio::time::timeout(wait, self.sessions.wait())
      .await
      .is_err()
    {
      warn!(
        sockets = self.sessions.len(),
        "agent sockets still open at shutdown"
      );
    }
  }

  /// Disconnects each logged-in agent that [`Controller::admits`] no more, and deletes the kept
  /// index of each agent no longer configured.
  fn drop_unconfigured(&self) {
    let options = self.config.current();
    let unconfigured: Vec<(String, u64)> = lock(&self.registry)
      .agents
      .iter()
      .filter(|(name, r)| Self::admits(&options.values, name, r).is_err())
      .map(|(name, r)| (name.clone(), r.socket))
      .collect();
    for (name, socket) in unconfigured {
      info!(
        agent = name,
        "agent's configuration changed; disconnecting it"
      );
      self.leave(&name, socket, close::KICKED, "configuration changed");
    }
    for name in self.shares.away_hosts() {
      if Self::configured(&options.values, &name).is_none() {
        self.shares.forget_away(&name);
      }
    }
  }

  /// A number for a socket that opened, its id while it lives.
  fn next_socket(&self) -> u64 {
    self.sockets.fetch_add(1, Ordering::Relaxed) + 1
  }
}

/// Takes the registration of the agent `name` out of `registry` when it is on `socket` and not
/// a newer one, with its share tokens, and the waits on it.
fn take(registry: &mut Registry, name: &str, socket: u64) -> Option<Left> {
  if registry.agents.get(name).is_none_or(|r| r.socket != socket) {
    return None;
  }
  let registration = registry
    .agents
    .remove(name)
    .expect("the registration was just found");
  registry.share_tokens.retain(|_, t| t.agent != name);
  let infos: Vec<Uuid> = registry
    .file_infos
    .iter()
    .filter(|(_, w)| w.agent == name)
    .map(|(id, _)| *id)
    .collect();
  let streams: Vec<Uuid> = registry
    .streams
    .iter()
    .filter(|(_, w)| w.agent == name)
    .map(|(id, _)| *id)
    .collect();
  Some(Left {
    registration,
    infos: infos
      .iter()
      .filter_map(|id| registry.file_infos.remove(id))
      .collect(),
    streams: streams
      .iter()
      .filter_map(|id| registry.streams.remove(id))
      .collect(),
  })
}
