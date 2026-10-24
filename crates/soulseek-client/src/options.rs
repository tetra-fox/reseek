//! The client's options: what it listens on, how each kind of connection behaves, the protocol
//! version it logs in with, the limits that bound remote input, and the application's resolvers.
//! Options are validated when the client is built and whenever a patch replaces them.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use soulseek_proto::{Limits, PROTOCOL_MAJOR_VERSION, PROTOCOL_MINOR_VERSION, Secret};
use tokio::sync::Semaphore;

use crate::error::ClientError;
use crate::resolvers::Resolvers;

/// Everything that configures a [`Client`](crate::Client)
#[derive(Clone, Debug)]
pub struct ClientOptions {
  /// accept connections from peers on `listen_ip_address:listen_port` while connected
  pub enable_listener: bool,
  pub listen_ip_address: IpAddr,
  /// also the port the server tells other users to connect to, whether or not we listen
  pub listen_port: u16,
  /// TCP accept backlog of the listener
  pub listen_backlog: u32,
  /// look for a parent in the distributed search tree and pass searches on to children
  pub enable_distributed_network: bool,
  /// let other users join the distributed search tree below us
  pub accept_distributed_children: bool,
  /// most children we keep at once
  pub distributed_child_limit: u32,
  /// searches we run at once; fixed for the client's lifetime
  pub maximum_concurrent_searches: u32,
  /// uploads we run at once; fixed for the client's lifetime
  pub maximum_concurrent_uploads: u32,
  /// KiB/s across all uploads
  pub maximum_upload_speed: u32,
  /// downloads we run at once; fixed for the client's lifetime
  pub maximum_concurrent_downloads: u32,
  /// KiB/s across all downloads
  pub maximum_download_speed: u32,
  /// drop a distributed search identical to the one received just before it
  pub deduplicate_search_requests: bool,
  /// how long a request waits for the reply that answers it; fixed for the client's lifetime
  pub message_timeout: Duration,
  /// acknowledge private messages as soon as they arrive, so the server stops replaying them
  pub auto_acknowledge_private_messages: bool,
  /// acknowledge privilege notifications as soon as they arrive
  pub auto_acknowledge_privilege_notifications: bool,
  /// let other users add us to their private rooms
  pub accept_private_room_invitations: bool,
  /// the first token handed out; fixed for the client's lifetime
  pub starting_token: u32,
  /// the server connection. Its inactivity timeout is always off (the server can be silent for
  /// a long time) and its keep-alive is always `server_keepalive`.
  pub server_connection_options: ConnectionOptions,
  /// peer message connections, outbound and inbound once identified
  pub peer_connection_options: ConnectionOptions,
  /// file transfer connections; the connect timeout also bounds how long a download waits for
  /// the uploader to connect to us
  pub transfer_connection_options: ConnectionOptions,
  /// an accepted connection until its first message says what it is for
  pub incoming_connection_options: ConnectionOptions,
  /// parent and child connections in the distributed search tree
  pub distributed_connection_options: ConnectionOptions,
  /// TCP keep-alive on the server socket. The server never answers a ping, so this is how a dead
  /// server connection is noticed: after about `idle + probes * interval`.
  pub server_keepalive: KeepAlive,
  /// the major version sent at login; fixed for the client's lifetime
  pub protocol_major_version: u32,
  /// the minor version sent at login; fixed for the client's lifetime
  pub protocol_minor_version: u32,
  /// log, at debug, server messages that carry bytes past the fields we read
  pub report_unread_message_data: bool,
  /// bounds on remote input and on the work it can start
  pub limits: ClientLimits,
  /// the application's callbacks and caches
  pub resolvers: Resolvers,
}

impl Default for ClientOptions {
  fn default() -> Self {
    Self {
      enable_listener: true,
      listen_ip_address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
      listen_port: 50_000,
      listen_backlog: i32::MAX as u32,
      enable_distributed_network: true,
      accept_distributed_children: true,
      distributed_child_limit: 25,
      maximum_concurrent_searches: 2,
      maximum_concurrent_uploads: 10,
      maximum_upload_speed: i32::MAX as u32,
      maximum_concurrent_downloads: i32::MAX as u32,
      maximum_download_speed: i32::MAX as u32,
      deduplicate_search_requests: true,
      message_timeout: Duration::from_secs(5),
      auto_acknowledge_private_messages: true,
      auto_acknowledge_privilege_notifications: true,
      accept_private_room_invitations: false,
      starting_token: 0,
      server_connection_options: ConnectionOptions {
        inactivity_timeout: None,
        ..ConnectionOptions::default()
      },
      peer_connection_options: ConnectionOptions::default(),
      transfer_connection_options: ConnectionOptions::default(),
      incoming_connection_options: ConnectionOptions::default(),
      distributed_connection_options: ConnectionOptions::default(),
      server_keepalive: KeepAlive {
        enabled: true,
        idle: Duration::from_secs(10),
        interval: Duration::from_secs(2),
        probes: 3,
      },
      protocol_major_version: PROTOCOL_MAJOR_VERSION,
      protocol_minor_version: PROTOCOL_MINOR_VERSION,
      report_unread_message_data: false,
      limits: ClientLimits::default(),
      resolvers: Resolvers::default(),
    }
  }
}

impl ClientOptions {
  /// checks every field, failing on the first one out of range
  pub fn validate(&self) -> Result<(), ClientError> {
    if self.listen_port < 1024 {
      return Err(invalid(
        "listen_port",
        format!(
          "Must be between 1024 and 65535 (specified: {})",
          self.listen_port
        ),
      ));
    }
    if !(128..=i32::MAX as u32).contains(&self.listen_backlog) {
      return Err(invalid(
        "listen_backlog",
        format!(
          "Must be greater than or equal to 128 and at most {} (specified: {})",
          i32::MAX,
          self.listen_backlog
        ),
      ));
    }
    at_least_one(
      "maximum_concurrent_searches",
      self.maximum_concurrent_searches,
    )?;
    at_least_one(
      "maximum_concurrent_uploads",
      self.maximum_concurrent_uploads,
    )?;
    at_least_one(
      "maximum_concurrent_downloads",
      self.maximum_concurrent_downloads,
    )?;
    if self.message_timeout.is_zero() {
      return Err(invalid("message_timeout", "Must be greater than zero"));
    }
    self
      .server_connection_options
      .validate("server_connection_options")?;
    self
      .peer_connection_options
      .validate("peer_connection_options")?;
    self
      .transfer_connection_options
      .validate("transfer_connection_options")?;
    self
      .incoming_connection_options
      .validate("incoming_connection_options")?;
    self
      .distributed_connection_options
      .validate("distributed_connection_options")?;
    self.server_keepalive.validate("server_keepalive")?;
    if self.protocol_major_version == 0 {
      return Err(invalid(
        "protocol_major_version",
        "Must be greater than zero",
      ));
    }
    if self.protocol_minor_version <= 100 {
      return Err(invalid(
        "protocol_minor_version",
        format!(
          "Must be greater than 100 (specified: {})",
          self.protocol_minor_version
        ),
      ));
    }
    self.limits.validate()
  }

  /// the options the server connection is actually created with
  pub(crate) fn effective_server_connection_options(&self) -> ConnectionOptions {
    ConnectionOptions {
      inactivity_timeout: None,
      keepalive: self.server_keepalive.clone(),
      ..self.server_connection_options.clone()
    }
  }
}

/// How one kind of connection reads, writes, times out and reaches its remote
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionOptions {
  /// bytes per socket read
  pub read_buffer_size: usize,
  /// bytes per socket write
  pub write_buffer_size: usize,
  /// writes that may be waiting on the connection; one more closes it as dead
  pub write_queue_size: u32,
  /// one deadline for the TCP connect and the proxy handshake together
  pub connect_timeout: Duration,
  /// how long the connection may go without a byte read or written before it is closed; `None`
  /// never closes it for inactivity
  pub inactivity_timeout: Option<Duration>,
  /// reach outbound remotes through this SOCKS5 proxy
  pub proxy: Option<ProxyOptions>,
  /// TCP keep-alive on sockets made with these options
  pub keepalive: KeepAlive,
  /// `TCP_MAXSEG` on sockets made with these options, where the platform lets it be set
  pub max_segment_size: Option<u32>,
}

impl Default for ConnectionOptions {
  fn default() -> Self {
    Self {
      read_buffer_size: 16_384,
      write_buffer_size: 16_384,
      write_queue_size: 250,
      connect_timeout: Duration::from_secs(10),
      inactivity_timeout: Some(Duration::from_secs(15)),
      proxy: None,
      keepalive: KeepAlive::default(),
      max_segment_size: None,
    }
  }
}

impl ConnectionOptions {
  fn validate(&self, name: &str) -> Result<(), ClientError> {
    if self.read_buffer_size == 0 {
      return Err(invalid_in(
        name,
        "read_buffer_size",
        "Must be greater than zero",
      ));
    }
    if self.write_buffer_size == 0 {
      return Err(invalid_in(
        name,
        "write_buffer_size",
        "Must be greater than zero",
      ));
    }
    // the write queue is a channel of this capacity, which tokio caps
    if self.write_queue_size == 0 || self.write_queue_size as usize > Semaphore::MAX_PERMITS {
      return Err(invalid_in(
        name,
        "write_queue_size",
        format!(
          "Must be between 1 and {} (specified: {})",
          Semaphore::MAX_PERMITS,
          self.write_queue_size
        ),
      ));
    }
    if self.connect_timeout.is_zero() {
      return Err(invalid_in(
        name,
        "connect_timeout",
        "Must be greater than zero",
      ));
    }
    if self
      .inactivity_timeout
      .is_some_and(|timeout| timeout.is_zero())
    {
      return Err(invalid_in(
        name,
        "inactivity_timeout",
        "Must be greater than zero, or unset to disable it",
      ));
    }
    if let Some(proxy) = &self.proxy {
      proxy.validate(name)?;
    }
    self.keepalive.validate(&format!("{name}.keepalive"))
  }
}

/// TCP keep-alive settings for a socket
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeepAlive {
  pub enabled: bool,
  /// silence before the first probe; whole seconds
  pub idle: Duration,
  /// time between probes; whole seconds
  pub interval: Duration,
  /// unanswered probes before the connection is dropped
  pub probes: u32,
}

impl KeepAlive {
  fn validate(&self, name: &str) -> Result<(), ClientError> {
    if !self.enabled {
      return Ok(());
    }
    if self.idle < Duration::from_secs(1) {
      return Err(invalid_in(name, "idle", "Must be at least 1 second"));
    }
    if self.interval < Duration::from_secs(1) {
      return Err(invalid_in(name, "interval", "Must be at least 1 second"));
    }
    if self.probes == 0 {
      return Err(invalid_in(name, "probes", "Must be at least 1"));
    }
    Ok(())
  }
}

/// A SOCKS5 proxy. The address is resolved on every connect, so a changed DNS record is picked up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyOptions {
  /// hostname or IP address
  pub address: String,
  pub port: u16,
  pub credentials: Option<ProxyCredentials>,
}

impl ProxyOptions {
  fn validate(&self, name: &str) -> Result<(), ClientError> {
    if self.address.trim().is_empty() {
      return Err(invalid_in(
        name,
        "proxy.address",
        "Must not be empty or whitespace",
      ));
    }
    if let Some(credentials) = &self.credentials {
      // SOCKS5 sends each as a one-byte length and the bytes
      for (field, value) in [
        ("proxy.username", credentials.username.as_str()),
        ("proxy.password", credentials.password.expose()),
      ] {
        if !(1..=255).contains(&value.len()) {
          return Err(invalid_in(name, field, "Must be between 1 and 255 bytes"));
        }
      }
    }
    Ok(())
  }
}

/// The username and password a SOCKS5 proxy asks for
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyCredentials {
  pub username: String,
  pub password: Secret,
}

/// Bounds on remote input and on the work it can make the client start
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientLimits {
  /// largest frame accepted on a peer connection; browse responses of large shares are the
  /// only legitimately big ones
  pub max_message_size: u32,
  /// outbound peer connection attempts in flight at once
  pub max_concurrent_peer_connects: u32,
  /// peer message connections kept open at once
  pub max_cached_peer_connections: u32,
  /// searches from other users we answer at once
  pub max_concurrent_search_responses: u32,
  /// undelivered search responses kept for a late connection from the searcher
  pub search_response_cache_size: u32,
  /// user addresses remembered between connects
  pub user_endpoint_cache_size: u32,
  /// how long a would-be parent has to send its branch level, root and a search
  pub distributed_parent_negotiation_timeout: Duration,
}

impl Default for ClientLimits {
  fn default() -> Self {
    Self {
      max_message_size: 256 << 20,
      max_concurrent_peer_connects: 50,
      max_cached_peer_connections: 1000,
      max_concurrent_search_responses: 20,
      search_response_cache_size: 1000,
      user_endpoint_cache_size: 10_000,
      distributed_parent_negotiation_timeout: Duration::from_secs(5),
    }
  }
}

impl ClientLimits {
  /// the frame and payload limits the decoders enforce
  pub fn frame_limits(&self) -> Limits {
    Limits {
      peer_message_bytes: self.max_message_size,
      ..Limits::DEFAULT
    }
  }

  fn validate(&self) -> Result<(), ClientError> {
    // a frame must at least hold its 4-byte code
    if self.max_message_size < 4 {
      return Err(invalid(
        "limits.max_message_size",
        "Must be at least 4 bytes",
      ));
    }
    at_least_one(
      "limits.max_concurrent_peer_connects",
      self.max_concurrent_peer_connects,
    )?;
    at_least_one(
      "limits.max_cached_peer_connections",
      self.max_cached_peer_connections,
    )?;
    at_least_one(
      "limits.max_concurrent_search_responses",
      self.max_concurrent_search_responses,
    )?;
    at_least_one(
      "limits.search_response_cache_size",
      self.search_response_cache_size,
    )?;
    at_least_one(
      "limits.user_endpoint_cache_size",
      self.user_endpoint_cache_size,
    )?;
    if self.distributed_parent_negotiation_timeout.is_zero() {
      return Err(invalid(
        "limits.distributed_parent_negotiation_timeout",
        "Must be greater than zero",
      ));
    }
    Ok(())
  }
}

fn invalid(field: &str, message: impl AsRef<str>) -> ClientError {
  ClientError::InvalidArgument(format!("{field}: {}", message.as_ref()))
}

fn invalid_in(name: &str, field: &str, message: impl AsRef<str>) -> ClientError {
  invalid(&format!("{name}.{field}"), message)
}

fn at_least_one(field: &str, value: u32) -> Result<(), ClientError> {
  if value == 0 {
    return Err(invalid(field, "Must be greater than or equal to 1"));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn defaults_are_valid() {
    ClientOptions::default().validate().unwrap();
  }

  #[test]
  fn listen_port_and_backlog_ranges() {
    let options = ClientOptions {
      listen_port: 1023,
      ..ClientOptions::default()
    };
    assert!(matches!(
      options.validate(),
      Err(ClientError::InvalidArgument(message)) if message.starts_with("listen_port: Must be between 1024 and 65535")
    ));
    let options = ClientOptions {
      listen_backlog: 127,
      ..ClientOptions::default()
    };
    assert!(matches!(
      options.validate(),
      Err(ClientError::InvalidArgument(message)) if message.starts_with("listen_backlog: Must be greater than or equal to 128")
    ));
  }

  #[test]
  fn protocol_versions() {
    let options = ClientOptions {
      protocol_minor_version: 100,
      ..ClientOptions::default()
    };
    assert!(options.validate().is_err());
    let options = ClientOptions {
      protocol_major_version: 0,
      ..ClientOptions::default()
    };
    assert!(options.validate().is_err());
  }

  #[test]
  fn keepalive_is_checked_only_when_enabled() {
    let mut options = ClientOptions::default();
    options.server_keepalive.probes = 0;
    assert!(options.validate().is_err());
    options.server_keepalive.enabled = false;
    options.validate().unwrap();
  }

  #[test]
  fn proxy_credentials_fit_socks5() {
    let mut options = ClientOptions::default();
    options.peer_connection_options.proxy = Some(ProxyOptions {
      address: "proxy".into(),
      port: 1080,
      credentials: Some(ProxyCredentials {
        username: "u".repeat(256),
        password: Secret::new("p"),
      }),
    });
    assert!(matches!(
      options.validate(),
      Err(ClientError::InvalidArgument(message)) if message.starts_with("peer_connection_options.proxy.username")
    ));
  }

  #[test]
  fn server_connection_never_times_out_and_uses_the_server_keepalive() {
    let mut options = ClientOptions::default();
    options.server_connection_options.inactivity_timeout = Some(Duration::from_secs(1));
    let effective = options.effective_server_connection_options();
    assert_eq!(effective.inactivity_timeout, None);
    assert_eq!(effective.keepalive, options.server_keepalive);
  }

  #[test]
  fn max_message_size_is_the_peer_frame_limit() {
    let limits = ClientLimits {
      max_message_size: 1 << 20,
      ..ClientLimits::default()
    };
    assert_eq!(limits.frame_limits().peer_message_bytes, 1 << 20);
  }
}
