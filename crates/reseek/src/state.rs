//! The application state document and its one owner: every part of the daemon changes it
//! through [`StateOwner::update`], and every change is pushed to the event stream as
//! `application.state`.

use std::sync::Arc;

use api_types::live::LiveFrame;
use api_types::relay::RelayState;
use api_types::state::{
  ConfigurationState, ConnectionWatchdogState, DirectoriesState, DistributedNetworkState,
  HealthState, IncomingSearchHealth, ListenState, OwnStatus, PortMappingState, PortMappingStatus,
  Privileges, ProcessState, Reachable, SearchHealth, ServerConnectionState, ServerState,
  SharesState, State, UserState, UserStatistics,
};
use cascade::Loaded;
use tokio::sync::watch;

use crate::config::Options;
use crate::live::Live;
use crate::paths::Paths;
use crate::version;

/// The current state and the stream its changes are pushed to. Cloning gives another handle to
/// the same state.
#[derive(Clone)]
pub struct StateOwner {
  current: Arc<watch::Sender<Arc<State>>>,
  live: Live,
}

impl StateOwner {
  /// The state of a daemon that just started with `loaded` and `paths`, pushed to `live` on
  /// every change.
  pub fn new(loaded: &Loaded<Options>, paths: &Paths, live: Live) -> Self {
    Self {
      current: Arc::new(watch::Sender::new(Arc::new(initial(loaded, paths)))),
      live,
    }
  }

  /// The state as it is now.
  pub fn current(&self) -> Arc<State> {
    self.current.borrow().clone()
  }

  /// Changes the state with `change`. When the result differs, it replaces the state and is
  /// pushed; changes are made and pushed one at a time, in order.
  pub fn update(&self, change: impl FnOnce(&mut State)) {
    self.current.send_if_modified(|current| {
      let mut next = State::clone(current);
      change(&mut next);
      if next == **current {
        return false;
      }
      *current = Arc::new(next);
      self
        .live
        .publish(LiveFrame::ApplicationState(Box::new(State::clone(current))));
      true
    });
  }

  /// Takes the outcome of a configuration load: its reload error, what waits for a restart and
  /// the upload limit. What waits for a reconnect is the network's to say, since it depends on
  /// the connection.
  pub fn configured(&self, loaded: &Loaded<Options>) {
    self.update(|state| {
      state.configuration = configuration(loaded);
      state.pending_restart = !loaded.pending_restart.is_empty();
      state.process.upload_max_bytes = loaded.values.analysis.upload_max_bytes.as_u64();
    });
  }
}

/// The outcome of the last configuration reload: why it failed and when, both null once a
/// reload succeeds.
pub fn configuration(loaded: &Loaded<Options>) -> ConfigurationState {
  ConfigurationState {
    last_reload_error: loaded.load_error.clone(),
    last_reload_at: loaded.load_error_at,
  }
}

/// The state before anything has happened: not connected, no shares, nobody in the relay.
fn initial(loaded: &Loaded<Options>, paths: &Paths) -> State {
  State {
    version: version::info(),
    pending_reconnect: false,
    pending_restart: !loaded.pending_restart.is_empty(),
    server: ServerState {
      address: None,
      ip_end_point: None,
      state: ServerConnectionState::empty(),
      is_connected: false,
      is_connecting: false,
      is_logged_in: false,
      is_logging_in: false,
      is_transitioning: false,
      listen: ListenState {
        port: loaded.values.soulseek.listen_port,
        port_mapping: PortMappingState {
          state: PortMappingStatus::Off,
          method: None,
          external_address: None,
          external_port: None,
          error: None,
        },
        reachable: Reachable::Unknown,
      },
    },
    connection_watchdog: ConnectionWatchdogState {
      is_enabled: false,
      is_attempting_connection: false,
      next_attempt_at: None,
      is_login_unanswered: false,
    },
    relay: RelayState {
      enabled: loaded.values.relay.enabled,
      agents: Vec::new(),
    },
    health: HealthState {
      search: SearchHealth {
        incoming: IncomingSearchHealth {
          latency: 0.0,
          queue_depth: 0,
          drop_rate: 0.0,
        },
      },
    },
    user: UserState {
      username: None,
      status: OwnStatus::Online,
      privileges: Privileges {
        is_privileged: false,
        privileges_remaining: 0,
        privileged_until: None,
      },
      statistics: UserStatistics {
        average_speed: 0,
        directory_count: 0,
        file_count: 0,
        upload_count: 0,
      },
    },
    distributed_network: DistributedNetworkState {
      branch_level: 0,
      branch_root: None,
      can_accept_children: false,
      child_limit: 0,
      children: Vec::new(),
      has_parent: false,
      is_branch_root: false,
      parent: None,
    },
    shares: SharesState {
      scan_pending: false,
      scanning: false,
      ready: false,
      faulted: false,
      cancelled: false,
      scan_progress: 0.0,
      hosts: None,
      directories: 0,
      files: 0,
    },
    rooms: Vec::new(),
    configuration: configuration(loaded),
    directories: DirectoriesState {
      downloads: paths.downloads.display().to_string(),
      incomplete: paths.incomplete.display().to_string(),
    },
    started_at: chrono::Utc::now(),
    process: ProcessState {
      pid: std::process::id(),
      os: std::env::consts::OS.to_string(),
      arch: std::env::consts::ARCH.to_string(),
      memory_bytes: 0,
      auto_analysis_workers: crate::config::Workers::auto(),
      upload_max_bytes: loaded.values.analysis.upload_max_bytes.as_u64(),
    },
    listeners: Vec::new(),
  }
}
