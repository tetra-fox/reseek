//! The Soulseek network: the server, our account, the peer listener and connection tuning.

use std::path::PathBuf;
use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::DiagnosticLevel;

/// The Soulseek network.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Soulseek {
  /// The server's host name.
  #[cascade(
    env = "RESEEK_SLSK_ADDRESS",
    cli = "--slsk-address",
    apply = "reconnect"
  )]
  pub address: String,
  /// The server's port.
  #[cascade(env = "RESEEK_SLSK_PORT", cli = "--slsk-port", apply = "reconnect")]
  pub port: u16,
  /// The account to log in as; without it reseek does not connect.
  #[cascade(
    env = "RESEEK_SLSK_USERNAME",
    cli = "--slsk-username",
    apply = "reconnect"
  )]
  pub username: Option<String>,
  /// The account's password.
  #[cascade(
    env = "RESEEK_SLSK_PASSWORD",
    cli = "--slsk-password",
    apply = "reconnect",
    secret
  )]
  pub password: Option<String>,
  /// The profile text other users see.
  #[cascade(env = "RESEEK_SLSK_DESCRIPTION", cli = "--slsk-description")]
  pub description: String,
  /// The profile picture other users see, a JPEG, GIF or BMP file.
  #[cascade(env = "RESEEK_SLSK_PICTURE", cli = "--slsk-picture")]
  pub picture: Option<PathBuf>,
  /// The address the peer listener binds.
  #[cascade(
    env = "RESEEK_SLSK_LISTEN_IP_ADDRESS",
    cli = "--slsk-listen-ip-address"
  )]
  pub listen_ip_address: String,
  /// The port other users connect to us on.
  #[cascade(env = "RESEEK_SLSK_LISTEN_PORT", cli = "--slsk-listen-port")]
  pub listen_port: u16,
  /// The least severe protocol diagnostic logged: `none`, `warning`, `info` or `debug`.
  #[cascade(
    env = "RESEEK_SLSK_DIAG_LEVEL",
    cli = "--slsk-diag-level",
    apply = "restart"
  )]
  pub diagnostic_level: DiagnosticLevel,
  /// The distributed search network.
  pub distributed_network: DistributedNetwork,
  /// Connection tuning.
  pub connection: SoulseekConnection,
}

impl Default for Soulseek {
  fn default() -> Self {
    Self {
      address: "vps.slsknet.org".to_string(),
      port: 2271,
      username: None,
      password: None,
      description: "A reseek user.".to_string(),
      picture: None,
      listen_ip_address: "0.0.0.0".to_string(),
      listen_port: 50300,
      diagnostic_level: DiagnosticLevel::default(),
      distributed_network: DistributedNetwork::default(),
      connection: SoulseekConnection::default(),
    }
  }
}

/// The distributed search network.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct DistributedNetwork {
  /// Takes no part in the distributed search network.
  #[cascade(env = "RESEEK_SLSK_NO_DNET", cli = "--slsk-no-dnet")]
  pub disabled: bool,
  /// Refuses children in the distributed network.
  #[cascade(env = "RESEEK_SLSK_DNET_NO_CHILDREN", cli = "--slsk-dnet-no-children")]
  pub disable_children: bool,
  /// The most children we take.
  #[cascade(env = "RESEEK_SLSK_DNET_CHILDREN", cli = "--slsk-dnet-children")]
  pub child_limit: u32,
  /// Logs the distributed network's diagnostics.
  #[cascade(env = "RESEEK_SLSK_DNET_LOGGING", cli = "--slsk-dnet-logging")]
  pub logging: bool,
}

impl Default for DistributedNetwork {
  fn default() -> Self {
    Self {
      disabled: false,
      disable_children: false,
      child_limit: 25,
      logging: false,
    }
  }
}

/// Connection tuning.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekConnection {
  /// Timeouts.
  pub timeout: ConnectionTimeouts,
  /// Buffer and queue sizes.
  pub buffer: ConnectionBuffers,
  /// A SOCKS5 proxy for every outgoing connection.
  pub proxy: SoulseekProxy,
  /// The MTU of the server connection, from 576 to 65535; the system's when not set.
  #[cascade(
    env = "RESEEK_SLSK_SERVER_MTU",
    cli = "--slsk-server-mtu",
    apply = "reconnect"
  )]
  pub mtu: Option<u32>,
}

/// Connection timeouts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ConnectionTimeouts {
  /// How long a connection may take to open, at least a second.
  #[cascade(
    env = "RESEEK_SLSK_CONNECTION_TIMEOUT",
    cli = "--slsk-connection-timeout"
  )]
  #[serde(with = "humantime_serde")]
  pub connect: Duration,
  /// How long a peer connection may be idle before it is closed, at least a second.
  #[cascade(
    env = "RESEEK_SLSK_INACTIVITY_TIMEOUT",
    cli = "--slsk-inactivity-timeout"
  )]
  #[serde(with = "humantime_serde")]
  pub inactivity: Duration,
  /// How long a transfer may stall before it fails, at least 30 seconds.
  #[cascade(env = "RESEEK_SLSK_TRANSFER_TIMEOUT", cli = "--slsk-transfer-timeout")]
  #[serde(with = "humantime_serde")]
  pub transfer: Duration,
}

impl Default for ConnectionTimeouts {
  fn default() -> Self {
    Self {
      connect: Duration::from_secs(10),
      inactivity: Duration::from_secs(15),
      transfer: Duration::from_secs(60),
    }
  }
}

/// Buffer and queue sizes, in bytes unless they say otherwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ConnectionBuffers {
  /// The read buffer of a connection, at least 1024.
  #[cascade(env = "RESEEK_SLSK_READ_BUFFER", cli = "--slsk-read-buffer")]
  pub read: u32,
  /// The write buffer of a connection, at least 1024.
  #[cascade(env = "RESEEK_SLSK_WRITE_BUFFER", cli = "--slsk-write-buffer")]
  pub write: u32,
  /// The buffer of a transfer connection, at least 81920.
  #[cascade(
    env = "RESEEK_SLSK_TRANSFER_BUFFER",
    cli = "--slsk-transfer-buffer",
    apply = "restart"
  )]
  pub transfer: u32,
  /// How many writes may wait per connection, from 5 to 5000.
  #[cascade(env = "RESEEK_SLSK_WRITE_QUEUE", cli = "--slsk-write-queue")]
  pub write_queue: u32,
}

impl Default for ConnectionBuffers {
  fn default() -> Self {
    Self {
      read: 16384,
      write: 16384,
      transfer: 262_144,
      write_queue: 50,
    }
  }
}

/// A SOCKS5 proxy for every outgoing connection.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekProxy {
  /// Connects through the proxy.
  #[cascade(env = "RESEEK_SLSK_PROXY_ENABLED", cli = "--slsk-proxy")]
  pub enabled: bool,
  /// The proxy's host.
  #[cascade(env = "RESEEK_SLSK_PROXY_ADDRESS", cli = "--slsk-proxy-address")]
  pub address: Option<String>,
  /// The proxy's port.
  #[cascade(env = "RESEEK_SLSK_PROXY_PORT", cli = "--slsk-proxy-port")]
  pub port: Option<u16>,
  /// The username for the proxy.
  #[cascade(env = "RESEEK_SLSK_PROXY_USERNAME", cli = "--slsk-proxy-username")]
  pub username: Option<String>,
  /// The password for the proxy.
  #[cascade(
    env = "RESEEK_SLSK_PROXY_PASSWORD",
    cli = "--slsk-proxy-password",
    secret
  )]
  pub password: Option<String>,
}
