//! The controller: the agents logged in on their sockets, the tokens and waits of what it asked
//! them for, and what follows an agent's coming and going. An agent is registered under its
//! instance name once its login checks out; a second login under the same name closes the
//! first socket. Leaving, an agent takes its shares with it, and every wait on it fails at once.

mod downloads;
mod files;
mod login;
mod session;
mod shares;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use api_types::events::{Event, RelayAgentConnected, RelayAgentDisconnected};
use api_types::relay::{RelayAgentState, RelayFileUploadResult};
use cascade::Watcher;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, info_span, warn};
use uuid::Uuid;

use super::credential::{Key, Purpose};
use super::frames::close;
use crate::api::error::ApiError;
use crate::config::cidr::parse_trimmed_list;
use crate::config::{Options, RelayAgent};
use crate::events::EventBus;
use crate::services::AgentCredential;
use crate::shares::Shares;
use crate::state::StateOwner;
use crate::sync::lock;
use crate::telemetry::Telemetry;
use crate::telemetry::catalogue::RelayMetrics;
pub use files::RelayStream;
use files::{FileInfoWait, StreamWait};

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
}

/// A token handed to an agent, with what it may be used for.
struct Token {
  /// the agent it was handed to
  agent: String,
  /// the socket it was handed on
  socket: u64,
  /// the relative path of the download it is for, `None` for a share upload
  filename: Option<String>,
  expires: Instant,
}

/// Everything the controller keeps about its agents, under one lock.
#[derive(Default)]
struct Registry {
  agents: BTreeMap<String, Registration>,
  share_tokens: HashMap<Uuid, Token>,
  download_tokens: HashMap<Uuid, Token>,
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
  /// where uploaded share databases are written before they are loaded
  temporary: PathBuf,
  shutdown: CancellationToken,
  registry: Mutex<Registry>,
  sockets: AtomicU64,
  /// each configured agent's key, with the secret it was derived from
  keys: Mutex<HashMap<String, (String, Key)>>,
}

impl Controller {
  /// The controller serving the agents of the options, loading their shares into `shares`,
  /// raising its events on `events`, keeping `State.relay.agents` current and counting in
  /// `telemetry`. It follows the
  /// agents' configuration until `shutdown`, which closes every agent's socket.
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
      temporary: std::env::temp_dir().join("reseek"),
      shutdown,
      registry: Mutex::new(Registry::default()),
      sockets: AtomicU64::new(0),
      keys: Mutex::new(HashMap::new()),
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

  /// The key of the configured agent `name` with `secret`, derived once per secret.
  fn key(&self, name: &str, secret: &str) -> Key {
    let mut keys = lock(&self.keys);
    if let Some((derived_from, key)) = keys.get(name)
      && derived_from == secret
    {
      return key.clone();
    }
    let key = Key::derive(secret, name);
    keys.insert(name.to_string(), (secret.to_string(), key.clone()));
    key
  }

  /// Whether `address` is one the configured agent may connect from.
  fn address_allowed(agent: &RelayAgent, address: IpAddr) -> bool {
    // validated when the options were loaded, so a list that does not read allows nothing
    parse_trimmed_list(&agent.cidr).is_ok_and(|ranges| ranges.iter().any(|r| r.contains(address)))
  }

  /// Checks a call an agent makes over HTTP with `token` for `purpose`: it is logged in,
  /// configured, calling from one of its networks, and its credential is the one for the
  /// token. Each failure is a 401, logged.
  fn authenticate(
    &self,
    caller: &AgentCredential,
    purpose: Purpose,
    token: Uuid,
  ) -> Result<(), ApiError> {
    let name = caller.agent.as_str();
    if !lock(&self.registry).agents.contains_key(name) {
      warn!(
        agent = name,
        "relay call from an agent that is not logged in"
      );
      return Err(ApiError::unauthorized(format!("Agent {name} is not registered")).logged());
    }
    let options = self.config.current();
    let Some(agent) = Self::configured(&options.values, name) else {
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
      return Err(
        ApiError::unauthorized("the address is not one the agent may call from").logged(),
      );
    }
    // a configured agent has a secret, which loading the options checked
    let secret = agent.secret.as_deref().unwrap_or_default();
    if !self
      .key(name, secret)
      .verifies(purpose, &token.to_string(), &caller.credential)
    {
      warn!(agent = name, %token, "relay call with a credential mismatch");
      return Err(ApiError::unauthorized("the credential does not match").logged());
    }
    debug!(agent = name, %token, "Agent authenticated for token");
    Ok(())
  }

  /// Registers the agent `record` on `socket` in place of any earlier socket of the same name,
  /// which is closed, and raises `RelayAgentConnected`.
  fn register(&self, socket: u64, record: RelayAgentState, outbound: mpsc::Sender<Outbound>) {
    let name = record.name.clone();
    let earlier = lock(&self.registry).agents.get(&name).map(|r| r.socket);
    if let Some(earlier) = earlier {
      self.leave(&name, earlier, close::SUPERSEDED, "superseded");
    }
    let ip_address = record.ip_address.clone();
    lock(&self.registry).agents.insert(
      name.clone(),
      Registration {
        socket,
        record,
        outbound,
      },
    );
    self.publish();
    self
      .events
      .raise(Event::RelayAgentConnected(RelayAgentConnected {
        version: 0,
        agent: name,
        ip_address,
      }));
  }

  /// Ends the registration of the agent `name` on `socket`, closing it with `code`, when that
  /// socket is the one registered.
  fn leave(&self, name: &str, socket: u64, code: u16, reason: &'static str) {
    let outbound = lock(&self.registry)
      .agents
      .get(name)
      .filter(|r| r.socket == socket)
      .map(|r| r.outbound.clone());
    if let Some(outbound) = outbound {
      // a socket whose queue is full or closed is ending on its own and deregisters with this
      if let Err(e) = outbound.try_send(Outbound::Close(code, reason)) {
        debug!(agent = name, error = %e, "close not queued for the agent's socket");
      }
    }
    self.deregister(name, socket, reason);
  }

  /// Removes the agent `name` when it is registered on `socket` and not on a newer one: its
  /// shares are unloaded, every wait on it fails, and `RelayAgentDisconnected` is raised.
  fn deregister(&self, name: &str, socket: u64, reason: &str) {
    let failed = {
      let mut registry = lock(&self.registry);
      if registry.agents.get(name).is_none_or(|r| r.socket != socket) {
        return;
      }
      registry.agents.remove(name);
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
      let infos: Vec<_> = infos
        .iter()
        .filter_map(|id| registry.file_infos.remove(id))
        .collect();
      let streams: Vec<_> = streams
        .iter()
        .filter_map(|id| registry.streams.remove(id))
        .collect();
      (infos, streams)
    };
    let (infos, streams) = failed;
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

  /// Disconnects the agents whose configuration was removed or whose instance name or secret
  /// changed, until the daemon stops; then closes every agent's socket as shutting down.
  async fn follow_configuration(self: Arc<Self>) {
    let mut changes = self.config.changes();
    loop {
      let change = tokio::select! {
        () = self.shutdown.cancelled() => break,
        change = changes.recv() => change,
      };
      match change {
        Ok(_) | Err(RecvError::Lagged(_)) => self.drop_unconfigured(),
        Err(RecvError::Closed) => break,
      }
    }
    let agents: Vec<(String, u64)> = lock(&self.registry)
      .agents
      .iter()
      .map(|(name, r)| (name.clone(), r.socket))
      .collect();
    for (name, socket) in agents {
      self.leave(&name, socket, close::SHUTTING_DOWN, "shutting down");
    }
  }

  /// Disconnects each logged-in agent no longer configured with the secret it logged in with.
  fn drop_unconfigured(&self) {
    let options = self.config.current();
    let registered: Vec<(String, u64)> = lock(&self.registry)
      .agents
      .iter()
      .map(|(name, r)| (name.clone(), r.socket))
      .collect();
    for (name, socket) in registered {
      let secret = Self::configured(&options.values, &name).and_then(|agent| agent.secret);
      let kept = match (secret, lock(&self.keys).get(&name)) {
        (Some(secret), Some((derived_from, _))) => secret == *derived_from,
        _ => false,
      };
      if !kept {
        info!(
          agent = name,
          "agent's configuration changed; disconnecting it"
        );
        self.leave(&name, socket, close::KICKED, "configuration changed");
      }
    }
  }

  /// A number for a socket that opened, its id while it lives.
  fn next_socket(&self) -> u64 {
    self.sockets.fetch_add(1, Ordering::Relaxed) + 1
  }
}
