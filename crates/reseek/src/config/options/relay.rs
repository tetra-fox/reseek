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
  /// Enables relay mode.
  #[cascade(env = "RESEEK_RELAY", cli = "-r|--relay", apply = "restart")]
  pub enabled: bool,
  /// Relay role: `controller`, the one the daemon plays. The relay agent is the separate
  /// `reseek-agent` program.
  #[cascade(env = "RESEEK_RELAY_MODE", cli = "-m|--relay-mode", apply = "restart")]
  pub mode: RelayMode,
  /// Agents allowed to connect to this controller, by name.
  #[cascade(user_keyed)]
  pub agents: BTreeMap<String, RelayAgent>,
  /// Timeouts of a controller for responses from its agents.
  pub controller_timeouts: RelayTimeouts,
  /// Limits on what a controller accepts from its agents.
  pub limits: RelayLimits,
}

/// Timeouts of a controller for responses from its agents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct RelayTimeouts {
  /// Time an agent has to report whether a requested file exists and its size.
  #[cascade(env = "RESEEK_RELAY_CONTROLLER_TIMEOUTS_FILE_INFO")]
  #[serde(with = "crate::config::duration")]
  pub file_info: Duration,
  /// Time an agent has to acknowledge that it is sending a file a peer downloads.
  #[cascade(env = "RESEEK_RELAY_CONTROLLER_TIMEOUTS_FILE_ACK")]
  #[serde(with = "crate::config::duration")]
  pub file_ack: Duration,
  /// Time after the acknowledgment for the file data to start arriving.
  #[cascade(env = "RESEEK_RELAY_CONTROLLER_TIMEOUTS_FILE_STREAM")]
  #[serde(with = "crate::config::duration")]
  pub file_stream: Duration,
  /// Total time after login that an agent may defer sending its shares while it scans them.
  /// Past it, the agent is disconnected and reconnects.
  #[cascade(env = "RESEEK_RELAY_CONTROLLER_TIMEOUTS_SYNC_DEFERRAL")]
  #[serde(with = "crate::config::duration")]
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

/// Limits on what a controller accepts from its agents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct RelayLimits {
  /// Largest share database an agent may upload.
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

/// An agent allowed to connect to this controller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct RelayAgent {
  /// Instance name of the agent.
  #[cascade(secret)]
  pub instance_name: Option<String>,
  /// Secret the agent presents.
  #[cascade(secret)]
  pub secret: Option<String>,
  /// Networks the agent may connect from, comma-separated.
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
