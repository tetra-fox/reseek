//! Relay mode: one daemon on the network, the controller, serves the shares of others, its
//! agents, under its own login. An agent, the `reseek-agent` program, dials the controller's
//! socket, proves it holds its secret, uploads its share index, and sends the bytes of its files
//! over HTTP when a peer downloads one through the controller; it may also fetch the
//! controller's finished downloads.
//!
//! [`Relay`] holds the [`Controller`] when relay mode is on.

mod controller;
mod service;

use std::sync::Arc;

use cascade::Watcher;
use tokio_util::sync::CancellationToken;

pub use controller::{Controller, RelayStream};

use crate::config::Options;
use crate::events::EventBus;
use crate::paths::Paths;
use crate::shares::Shares;
use crate::state::StateOwner;
use crate::telemetry::Telemetry;

/// The relay as this daemon started it.
pub struct Relay {
  /// the controller, when relay mode is on
  pub controller: Option<Arc<Controller>>,
}

impl Relay {
  /// The controller when the options the daemon started with turn relay mode on.
  pub fn new(
    config: &Watcher<Options>,
    shares: &Arc<Shares>,
    events: &EventBus,
    state: &StateOwner,
    telemetry: &Arc<Telemetry>,
    paths: &Paths,
    shutdown: &CancellationToken,
  ) -> Arc<Self> {
    let controller = config.current().values.relay.enabled.then(|| {
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
    Arc::new(Self { controller })
  }
}
