//! The Soulseek network: the server, our account, the peer listener and connection tuning.

use std::path::PathBuf;
use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::DiagnosticLevel;

/// Soulseek network.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Soulseek {
  /// Host name of the Soulseek server.
  #[cascade(
    env = "RESEEK_SLSK_ADDRESS",
    cli = "--slsk-address",
    apply = "reconnect"
  )]
  pub address: String,
  /// Port of the Soulseek server.
  #[cascade(env = "RESEEK_SLSK_PORT", cli = "--slsk-port", apply = "reconnect")]
  pub port: u16,
  /// Soulseek username. Required to connect.
  #[cascade(
    env = "RESEEK_SLSK_USERNAME",
    cli = "--slsk-username",
    apply = "reconnect"
  )]
  pub username: Option<String>,
  /// Soulseek password.
  #[cascade(
    env = "RESEEK_SLSK_PASSWORD",
    cli = "--slsk-password",
    apply = "reconnect",
    secret
  )]
  pub password: Option<String>,
  /// Profile text shown to other users.
  #[cascade(env = "RESEEK_SLSK_DESCRIPTION", cli = "--slsk-description")]
  pub description: String,
  /// Profile picture shown to other users. JPEG, GIF or BMP. A relative path counts from the file
  /// that sets it.
  #[cascade(env = "RESEEK_SLSK_PICTURE", cli = "--slsk-picture", path)]
  pub picture: Option<PathBuf>,
  /// Your interests and dislikes, used to suggest similar users and interests.
  pub interests: SoulseekInterests,
  /// IP address on which to accept connections from other users.
  #[cascade(
    env = "RESEEK_SLSK_LISTEN_IP_ADDRESS",
    cli = "--slsk-listen-ip-address"
  )]
  pub listen_ip_address: String,
  /// Port on which to accept connections from other users.
  #[cascade(env = "RESEEK_SLSK_LISTEN_PORT", cli = "--slsk-listen-port")]
  pub listen_port: u16,
  /// Opens the listen port on the router with UPnP, NAT-PMP or PCP while connected.
  #[cascade(
    env = "RESEEK_SLSK_PORT_MAPPING",
    cli = "--slsk-no-port-mapping",
    negated
  )]
  pub port_mapping: bool,
  /// Minimum severity of logged protocol diagnostics: `none`, `warning`, `info` or `debug`.
  #[cascade(
    env = "RESEEK_SLSK_DIAG_LEVEL",
    cli = "--slsk-diag-level",
    apply = "restart"
  )]
  pub diagnostic_level: DiagnosticLevel,
  /// Distributed search network.
  pub distributed_network: DistributedNetwork,
  /// Connection settings.
  pub connection: SoulseekConnection,
  /// TCP keep-alive on the server connection. The server does not answer pings, so the probes
  /// are what detect a dead connection, after about `idle + probes * interval`.
  pub server_keepalive: ServerKeepalive,
  /// Limits on what other users can cause to be held or started.
  pub limits: SoulseekLimits,
  /// Outgoing searches.
  pub search: SoulseekSearch,
  /// Browsing the shares of other users.
  pub browse: SoulseekBrowse,
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
      interests: SoulseekInterests::default(),
      listen_ip_address: "0.0.0.0".to_string(),
      listen_port: 50300,
      port_mapping: true,
      diagnostic_level: DiagnosticLevel::default(),
      distributed_network: DistributedNetwork::default(),
      connection: SoulseekConnection::default(),
      server_keepalive: ServerKeepalive::default(),
      limits: SoulseekLimits::default(),
      search: SoulseekSearch::default(),
      browse: SoulseekBrowse::default(),
    }
  }
}

/// Your interests and dislikes, sent to the server at login and whenever they change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekInterests {
  /// Interests, in lowercase, such as `jazz`.
  #[cascade(env = "RESEEK_SLSK_INTERESTS_LIKED", cli = "--slsk-interests-liked")]
  pub liked: Vec<String>,
  /// Dislikes, in lowercase. An interest is in one of the two lists at most.
  #[cascade(env = "RESEEK_SLSK_INTERESTS_HATED", cli = "--slsk-interests-hated")]
  pub hated: Vec<String>,
}

/// Distributed search network.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct DistributedNetwork {
  /// Turns off the distributed network, which passes searches between users.
  #[cascade(env = "RESEEK_SLSK_NO_DNET", cli = "--slsk-no-dnet")]
  pub disabled: bool,
  /// Stops passing searches on to other users in the distributed network.
  #[cascade(env = "RESEEK_SLSK_DNET_NO_CHILDREN", cli = "--slsk-dnet-no-children")]
  pub disable_children: bool,
  /// Maximum number of users to pass searches on to in the distributed network.
  #[cascade(env = "RESEEK_SLSK_DNET_CHILDREN", cli = "--slsk-dnet-children")]
  pub child_limit: u32,
  /// Logs details of the distributed network.
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

/// Connection settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekConnection {
  /// Connection timeouts.
  pub timeout: ConnectionTimeouts,
  /// Buffer and queue sizes.
  pub buffer: ConnectionBuffers,
  /// SOCKS5 proxy for all outgoing connections.
  pub proxy: SoulseekProxy,
  /// MTU of the server connection, from 576 to 65535. The system default applies when not
  /// set.
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
  /// Maximum time to open a connection. Minimum: 1 second.
  #[cascade(
    env = "RESEEK_SLSK_CONNECTION_TIMEOUT",
    cli = "--slsk-connection-timeout"
  )]
  #[serde(with = "crate::config::duration")]
  pub connect: Duration,
  /// Idle time after which a peer connection is closed. Minimum: 1 second.
  #[cascade(
    env = "RESEEK_SLSK_INACTIVITY_TIMEOUT",
    cli = "--slsk-inactivity-timeout"
  )]
  #[serde(with = "crate::config::duration")]
  pub inactivity: Duration,
  /// Time a transfer may stall before it fails. Minimum: 30 seconds.
  #[cascade(env = "RESEEK_SLSK_TRANSFER_TIMEOUT", cli = "--slsk-transfer-timeout")]
  #[serde(with = "crate::config::duration")]
  pub transfer: Duration,
  /// Time another user has to answer a transfer request, for uploads and for `transfer_request`
  /// downloads. Minimum: 1 second.
  #[serde(with = "crate::config::duration")]
  pub transfer_response: Duration,
}

impl Default for ConnectionTimeouts {
  fn default() -> Self {
    Self {
      connect: Duration::from_secs(10),
      inactivity: Duration::from_secs(15),
      transfer: Duration::from_secs(60),
      transfer_response: Duration::from_secs(15),
    }
  }
}

/// Buffer and queue sizes, in bytes unless stated otherwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ConnectionBuffers {
  /// Read buffer size per connection, in bytes. Minimum: 1024.
  #[cascade(env = "RESEEK_SLSK_READ_BUFFER", cli = "--slsk-read-buffer")]
  pub read: u32,
  /// Write buffer size per connection, in bytes. Minimum: 1024.
  #[cascade(env = "RESEEK_SLSK_WRITE_BUFFER", cli = "--slsk-write-buffer")]
  pub write: u32,
  /// Buffer size per transfer connection, in bytes. Minimum: 81920. A change applies to
  /// transfers started after it.
  #[cascade(env = "RESEEK_SLSK_TRANSFER_BUFFER", cli = "--slsk-transfer-buffer")]
  pub transfer: u32,
  /// Maximum number of pending writes per connection, from 5 to 5000.
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

/// SOCKS5 proxy for all outgoing connections.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekProxy {
  /// Routes outgoing connections through the proxy.
  #[cascade(env = "RESEEK_SLSK_PROXY_ENABLED", cli = "--slsk-proxy")]
  pub enabled: bool,
  /// Host name of the proxy.
  #[cascade(env = "RESEEK_SLSK_PROXY_ADDRESS", cli = "--slsk-proxy-address")]
  pub address: Option<String>,
  /// Port of the proxy.
  #[cascade(env = "RESEEK_SLSK_PROXY_PORT", cli = "--slsk-proxy-port")]
  pub port: Option<u16>,
  /// Proxy username.
  #[cascade(env = "RESEEK_SLSK_PROXY_USERNAME", cli = "--slsk-proxy-username")]
  pub username: Option<String>,
  /// Proxy password.
  #[cascade(
    env = "RESEEK_SLSK_PROXY_PASSWORD",
    cli = "--slsk-proxy-password",
    secret
  )]
  pub password: Option<String>,
}

/// TCP keep-alive on the server connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ServerKeepalive {
  /// Sends TCP keep-alive probes on the server connection.
  #[cascade(env = "RESEEK_SLSK_SERVER_KEEPALIVE_ENABLED", apply = "reconnect")]
  pub enabled: bool,
  /// Idle time before the first probe. Minimum: 1 second.
  #[cascade(env = "RESEEK_SLSK_SERVER_KEEPALIVE_IDLE", apply = "reconnect")]
  #[serde(with = "crate::config::duration")]
  pub idle: Duration,
  /// Interval between probes. Minimum: 1 second.
  #[cascade(env = "RESEEK_SLSK_SERVER_KEEPALIVE_INTERVAL", apply = "reconnect")]
  #[serde(with = "crate::config::duration")]
  pub interval: Duration,
  /// Number of unanswered probes after which the connection is dropped. Minimum: 1.
  #[cascade(env = "RESEEK_SLSK_SERVER_KEEPALIVE_PROBES", apply = "reconnect")]
  pub probes: u32,
}

impl Default for ServerKeepalive {
  fn default() -> Self {
    Self {
      enabled: true,
      idle: Duration::from_secs(10),
      interval: Duration::from_secs(2),
      probes: 3,
    }
  }
}

/// Limits on what other users can cause to be held or started.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekLimits {
  /// Largest file list accepted from another user. Other messages have smaller limits of their
  /// own.
  #[cascade(kind = "size")]
  pub max_message_size: bytesize::ByteSize,
  /// Maximum number of connections to other users being opened at once.
  pub max_concurrent_peer_connects: u32,
  /// Maximum number of message connections to other users kept open.
  pub max_cached_peer_connections: u32,
  /// Maximum number of search results held for users who can't be reached yet.
  pub search_response_cache_size: u32,
  /// Maximum number of user addresses remembered between connections.
  pub user_endpoint_cache_size: u32,
  /// Time a user offering to pass you searches in the distributed network has to start doing so.
  /// One that doesn't is dropped.
  #[serde(with = "crate::config::duration")]
  pub distributed_parent_negotiation_timeout: Duration,
}

impl Default for SoulseekLimits {
  fn default() -> Self {
    Self {
      max_message_size: bytesize::ByteSize::mib(256),
      max_concurrent_peer_connects: 50,
      max_cached_peer_connections: 1000,
      search_response_cache_size: 1000,
      user_endpoint_cache_size: 10_000,
      distributed_parent_negotiation_timeout: Duration::from_secs(5),
    }
  }
}

/// Outgoing searches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekSearch {
  /// Maximum number of outgoing searches run at once. Further searches wait. Minimum: 1.
  #[cascade(apply = "restart")]
  pub max_concurrent: u32,
}

impl Default for SoulseekSearch {
  fn default() -> Self {
    Self { max_concurrent: 2 }
  }
}

/// Browsing the shares of other users.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekBrowse {
  /// Time a user has to start sending their file list when browsed. Minimum: 1 second.
  #[serde(with = "crate::config::duration")]
  pub response_timeout: Duration,
}

impl Default for SoulseekBrowse {
  fn default() -> Self {
    Self {
      response_timeout: Duration::from_secs(60),
    }
  }
}
