//! Relay mode: a controller that fronts agents holding shares on other machines.

use std::collections::BTreeMap;
use std::time::Duration;

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
  /// How long a controller waits for its agents.
  pub controller_timeouts: RelayTimeouts,
  /// What a controller accepts from its agents.
  pub limits: RelayLimits,
}

/// How long a controller waits for its agents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct RelayTimeouts {
  /// For an agent to say whether a file a peer asked for exists and how long it is.
  #[cascade(env = "RESEEK_RELAY_CONTROLLER_TIMEOUTS_FILE_INFO")]
  #[serde(with = "humantime_serde")]
  pub file_info: Duration,
  /// For an agent to acknowledge that it is sending a file a peer downloads.
  #[cascade(env = "RESEEK_RELAY_CONTROLLER_TIMEOUTS_FILE_ACK")]
  #[serde(with = "humantime_serde")]
  pub file_ack: Duration,
  /// After the acknowledgment, for the file's bytes to start arriving.
  #[cascade(env = "RESEEK_RELAY_CONTROLLER_TIMEOUTS_FILE_STREAM")]
  #[serde(with = "humantime_serde")]
  pub file_stream: Duration,
  /// In all, after an agent's login, while the agent says its shares are being scanned, for it
  /// to start sending them; past it the agent is disconnected and dials again.
  #[cascade(env = "RESEEK_RELAY_CONTROLLER_TIMEOUTS_SYNC_DEFERRAL")]
  #[serde(with = "humantime_serde")]
  pub sync_deferral: Duration,
}

impl Default for RelayTimeouts {
  fn default() -> Self {
    Self {
      file_info: Duration::from_secs(3),
      file_ack: Duration::from_secs(5),
      file_stream: Duration::from_secs(30),
      sync_deferral: Duration::from_secs(12 * 3600),
    }
  }
}

/// What a controller accepts from its agents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct RelayLimits {
  /// The largest share database an agent may upload.
  #[cascade(env = "RESEEK_RELAY_LIMITS_SHARE_DATABASE_MAX", kind = "size")]
  pub share_database_max: bytesize::ByteSize,
}

impl Default for RelayLimits {
  fn default() -> Self {
    Self {
      share_database_max: bytesize::ByteSize::gib(2),
    }
  }
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
  /// Glob patterns on a finished download's path under the controller's downloads directory,
  /// written with `/` between its parts; only the downloads one matches are fetched, every one
  /// when empty.
  #[cascade(env = "RESEEK_CONTROLLER_DOWNLOADS_FILTER")]
  pub downloads_filter: Vec<String>,
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
