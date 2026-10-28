//! Relay mode: one daemon on the network, the controller, serves the shares of others, its
//! agents, under its own login. An agent dials the controller's socket, proves it holds its
//! secret, uploads its share index, and sends the bytes of its files over HTTP when a peer
//! downloads one through the controller; it may also fetch the controller's finished downloads.
//! A debug instance is both, serving itself.
//!
//! [`Relay`] holds the half or halves the daemon started as: the [`Controller`] and the
//! [`AgentClient`].

mod agent;
mod controller;
pub mod credential;
mod frames;
mod service;

use std::sync::Arc;

use cascade::Watcher;
use tokio_util::sync::CancellationToken;

pub use agent::AgentClient;
pub use controller::{Controller, RelayStream};

use crate::config::Options;
use crate::config::enums::RelayMode;
use crate::events::EventBus;
use crate::paths::Paths;
use crate::shares::Shares;
use crate::state::StateOwner;
use crate::telemetry::Telemetry;

/// The relay halves this daemon runs, as it started.
pub struct Relay {
  /// the controller, in controller and debug mode
  pub controller: Option<Arc<Controller>>,
  /// the connection to a controller, in agent and debug mode
  pub agent: Option<Arc<AgentClient>>,
}

impl Relay {
  /// The halves the options the daemon started with ask for, none when the relay is off.
  pub fn new(
    config: &Watcher<Options>,
    shares: &Arc<Shares>,
    events: &EventBus,
    state: &StateOwner,
    telemetry: &Arc<Telemetry>,
    paths: &Paths,
    shutdown: &CancellationToken,
  ) -> Arc<Self> {
    let startup = config.current();
    let relay = &startup.values.relay;
    let (controls, connects) = match (relay.enabled, relay.mode) {
      (false, _) => (false, false),
      (true, RelayMode::Controller) => (true, false),
      (true, RelayMode::Agent) => (false, true),
      (true, RelayMode::Debug) => (true, true),
    };
    let controller = controls.then(|| {
      Controller::new(
        config.clone(),
        shares.clone(),
        events.clone(),
        state.clone(),
        telemetry.clone(),
        paths.downloads.clone(),
        shutdown.clone(),
      )
    });
    let agent = connects.then(|| {
      AgentClient::new(
        config.clone(),
        shares.clone(),
        state.clone(),
        paths.downloads.clone(),
        relay.mode == RelayMode::Debug,
        shutdown.clone(),
      )
    });
    Arc::new(Self { controller, agent })
  }
}
