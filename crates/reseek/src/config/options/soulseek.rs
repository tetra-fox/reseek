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
  /// Asks the router to forward `listen_port` to this computer, by UPnP, NAT-PMP or PCP, while
  /// connected to the server.
  #[cascade(
    env = "RESEEK_SLSK_PORT_MAPPING",
    cli = "--slsk-no-port-mapping",
    negated
  )]
  pub port_mapping: bool,
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
  /// TCP keep-alive on the server connection. The server never answers a ping, so this is how
  /// a dead server connection is noticed: after about `idle + probes * interval`.
  pub server_keepalive: ServerKeepalive,
  /// Bounds on what other users can make us hold or start.
  pub limits: SoulseekLimits,
  /// Our own searches.
  pub search: SoulseekSearch,
  /// Browsing other users' shares.
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
  /// How long an upload, or a download asked for with a transfer request, waits for the peer
  /// to answer the request, at least a second.
  #[serde(with = "humantime_serde")]
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
  /// The buffer of a transfer connection, at least 81920; a change applies to the transfers
  /// that start after it.
  #[cascade(env = "RESEEK_SLSK_TRANSFER_BUFFER", cli = "--slsk-transfer-buffer")]
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

/// TCP keep-alive on the server connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ServerKeepalive {
  /// Sends keep-alive probes on the server connection.
  #[cascade(env = "RESEEK_SLSK_SERVER_KEEPALIVE_ENABLED", apply = "reconnect")]
  pub enabled: bool,
  /// How long the connection is silent before the first probe, at least a second.
  #[cascade(env = "RESEEK_SLSK_SERVER_KEEPALIVE_IDLE", apply = "reconnect")]
  #[serde(with = "humantime_serde")]
  pub idle: Duration,
  /// The time between probes, at least a second.
  #[cascade(env = "RESEEK_SLSK_SERVER_KEEPALIVE_INTERVAL", apply = "reconnect")]
  #[serde(with = "humantime_serde")]
  pub interval: Duration,
  /// How many probes go unanswered before the connection is dropped, at least 1.
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

/// Bounds on what other users can make us hold or start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekLimits {
  /// The largest message accepted from another user; a large share's browse listing is the only
  /// message that is legitimately big.
  #[cascade(kind = "size")]
  pub max_message_size: bytesize::ByteSize,
  /// How many connections to other users are being opened at once.
  pub max_concurrent_peer_connects: u32,
  /// How many message connections to other users are kept open.
  pub max_cached_peer_connections: u32,
  /// How many searches from the distributed network are answered at once.
  pub max_concurrent_search_responses: u32,
  /// How many search responses are kept for a searcher we could not reach yet.
  pub search_response_cache_size: u32,
  /// How many users' addresses are remembered between connections.
  pub user_endpoint_cache_size: u32,
  /// How long a would-be distributed parent has to send its branch level, root and a search.
  #[serde(with = "humantime_serde")]
  pub distributed_parent_negotiation_timeout: Duration,
}

impl Default for SoulseekLimits {
  fn default() -> Self {
    Self {
      max_message_size: bytesize::ByteSize::mib(256),
      max_concurrent_peer_connects: 50,
      max_cached_peer_connections: 1000,
      max_concurrent_search_responses: 20,
      search_response_cache_size: 1000,
      user_endpoint_cache_size: 10_000,
      distributed_parent_negotiation_timeout: Duration::from_secs(5),
    }
  }
}

/// Our own searches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekSearch {
  /// How many of our searches run at once, at least 1; the others wait their turn.
  #[cascade(apply = "restart")]
  pub max_concurrent: u32,
}

impl Default for SoulseekSearch {
  fn default() -> Self {
    Self { max_concurrent: 2 }
  }
}

/// Browsing other users' shares.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SoulseekBrowse {
  /// How long a user has to start sending their share listing, at least a second; a large
  /// share takes a while to put together.
  #[serde(with = "humantime_serde")]
  pub response_timeout: Duration,
}

impl Default for SoulseekBrowse {
  fn default() -> Self {
    Self {
      response_timeout: Duration::from_secs(60),
    }
  }
}
