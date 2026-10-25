//! Relay mode: a controller that fronts agents holding shares on other machines.

use std::collections::BTreeMap;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::RelayMode;

/// Relay mode.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Relay {
  /// Turns relay mode on.
  #[cascade(env = "RESEEK_RELAY", cli = "-r|--relay", apply = "restart")]
  pub enabled: bool,
  /// `controller`, `agent`, or `debug` for a controller that is also its own agent.
  #[cascade(env = "RESEEK_RELAY_MODE", cli = "-m|--relay-mode", apply = "restart")]
  pub mode: RelayMode,
  /// The controller an agent connects to.
  pub controller: RelayController,
  /// The agents allowed to connect to this controller, by name.
  #[cascade(user_keyed)]
  pub agents: BTreeMap<String, RelayAgent>,
}

/// The controller an agent connects to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct RelayController {
  /// The controller's base URL, such as `https://host:5031`.
  #[cascade(env = "RESEEK_CONTROLLER_ADDRESS", cli = "--controller-address")]
  pub address: Option<String>,
  /// Accepts any TLS certificate from the controller.
  #[cascade(
    env = "RESEEK_CONTROLLER_IGNORE_CERTIFICATE_ERRORS",
    cli = "--controller-ignore-certificate-errors"
  )]
  pub ignore_certificate_errors: bool,
  /// One of the controller's API keys, with the read-write or administrator role.
  #[cascade(
    env = "RESEEK_CONTROLLER_API_KEY",
    cli = "--controller-api-key",
    secret
  )]
  pub api_key: Option<String>,
  /// The secret the controller has for this agent.
  #[cascade(env = "RESEEK_CONTROLLER_SECRET", cli = "--controller-secret", secret)]
  pub secret: Option<String>,
  /// Fetches finished downloads from the controller.
  #[cascade(env = "RESEEK_CONTROLLER_DOWNLOADS", cli = "--controller-downloads")]
  pub downloads: bool,
}

/// An agent allowed to connect to this controller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct RelayAgent {
  /// The agent's own instance name.
  #[cascade(secret)]
  pub instance_name: Option<String>,
  /// The secret the agent presents.
  #[cascade(secret)]
  pub secret: Option<String>,
  /// The addresses the agent may connect from, as comma-separated networks.
  pub cidr: String,
}

impl Default for RelayAgent {
  fn default() -> Self {
    Self {
      instance_name: None,
      secret: None,
      cidr: crate::config::cidr::ANY.to_string(),
    }
  }
}
