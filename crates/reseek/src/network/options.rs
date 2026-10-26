//! The client's options as the configuration gives them. One mapping makes the options the
//! client starts with and the patch a configuration change applies, so a client that started with
//! some options and one patched to them run with the same ones.

use std::net::IpAddr;
use std::time::Duration;

use soulseek_client::{
  ClientLimits, ClientOptions, ClientOptionsPatch, ConnectionOptions, KeepAlive, ProxyCredentials,
  ProxyOptions, RequestStyle,
};
use soulseek_proto::Secret;

use crate::config::{Options, Soulseek, enums};

/// How many writes may wait on a server, peer or transfer connection. They are meant to have no
/// bound; this one only stops a stuck connection from growing memory without limit, and leaves
/// room for the distributed tree refusing a backlog of up to 1024 child requests at once on the
/// server connection.
pub const UNBOUNDED_WRITE_QUEUE: u32 = 10_000;

/// How long a request waits for the server's or a peer's reply.
const MESSAGE_TIMEOUT: Duration = Duration::from_secs(5);

/// The bytes of IPv4 and TCP headers, taken off the MTU for the server socket's segment size.
const HEADERS: u32 = 40;

/// The probes a distributed connection's keep-alive sends before it gives up.
const DISTRIBUTED_PROBES: u32 = 3;

/// The client options `options` call for, without the resolvers, which are the daemon's own
/// and never change. Fails only for options that did not pass validation.
pub fn client_options(options: &Options) -> Result<ClientOptions, String> {
  let soulseek = &options.soulseek;
  let transfers = &options.transfers;
  let listen_ip_address: IpAddr = soulseek
    .listen_ip_address
    .parse()
    .map_err(|e| format!("soulseek.listen_ip_address: {e}"))?;
  let sets = ConnectionSets::of(soulseek)?;
  let keepalive = &soulseek.server_keepalive;
  let limits = &soulseek.limits;
  Ok(ClientOptions {
    enable_listener: true,
    listen_ip_address,
    listen_port: soulseek.listen_port,
    enable_distributed_network: !soulseek.distributed_network.disabled,
    accept_distributed_children: !soulseek.distributed_network.disable_children,
    distributed_child_limit: soulseek.distributed_network.child_limit,
    maximum_concurrent_searches: soulseek.search.max_concurrent,
    maximum_concurrent_uploads: transfers.upload.slots,
    maximum_concurrent_downloads: transfers
      .download
      .slots
      .unwrap_or(ClientOptions::default().maximum_concurrent_downloads),
    // the client takes 0 as no limit
    maximum_upload_speed: transfers.upload.speed_limit.unwrap_or(0),
    maximum_download_speed: transfers.download.speed_limit.unwrap_or(0),
    deduplicate_search_requests: true,
    message_timeout: MESSAGE_TIMEOUT,
    transfer_response_timeout: soulseek.connection.timeout.transfer_response,
    // the conversations engine acknowledges a message once it is stored
    auto_acknowledge_private_messages: false,
    auto_acknowledge_privilege_notifications: true,
    accept_private_room_invitations: true,
    starting_token: None,
    server_connection_options: sets.server,
    peer_connection_options: sets.peer.clone(),
    transfer_connection_options: sets.transfer,
    incoming_connection_options: sets.peer,
    distributed_connection_options: sets.distributed,
    server_keepalive: KeepAlive {
      enabled: keepalive.enabled,
      idle: keepalive.idle,
      interval: keepalive.interval,
      probes: keepalive.probes,
    },
    limits: ClientLimits {
      max_message_size: u32::try_from(limits.max_message_size.as_u64())
        .map_err(|_| "soulseek.limits.max_message_size: more than 4 GiB".to_string())?,
      max_concurrent_peer_connects: limits.max_concurrent_peer_connects,
      max_cached_peer_connections: limits.max_cached_peer_connections,
      max_concurrent_search_responses: limits.max_concurrent_search_responses,
      search_response_cache_size: limits.search_response_cache_size,
      user_endpoint_cache_size: limits.user_endpoint_cache_size,
      distributed_parent_negotiation_timeout: limits.distributed_parent_negotiation_timeout,
    },
    ..ClientOptions::default()
  })
}

/// The patch that moves a client running with the options for `previous` to the options for
/// `current`: each option that differs, and the speed limits always.
pub fn patch(previous: &Options, current: &Options) -> Result<ClientOptionsPatch, String> {
  let from = client_options(previous)?;
  let to = client_options(current)?;
  fn changed<T: PartialEq>(from: T, to: T) -> Option<T> {
    (from != to).then_some(to)
  }
  Ok(ClientOptionsPatch {
    enable_listener: changed(from.enable_listener, to.enable_listener),
    listen_ip_address: changed(from.listen_ip_address, to.listen_ip_address),
    listen_port: changed(from.listen_port, to.listen_port),
    listen_backlog: changed(from.listen_backlog, to.listen_backlog),
    enable_distributed_network: changed(
      from.enable_distributed_network,
      to.enable_distributed_network,
    ),
    accept_distributed_children: changed(
      from.accept_distributed_children,
      to.accept_distributed_children,
    ),
    distributed_child_limit: changed(from.distributed_child_limit, to.distributed_child_limit),
    // a restart applies it; the client keeps the bound it started with
    maximum_concurrent_searches: None,
    maximum_concurrent_uploads: changed(
      from.maximum_concurrent_uploads,
      to.maximum_concurrent_uploads,
    ),
    maximum_upload_speed: Some(to.maximum_upload_speed),
    maximum_concurrent_downloads: changed(
      from.maximum_concurrent_downloads,
      to.maximum_concurrent_downloads,
    ),
    maximum_download_speed: Some(to.maximum_download_speed),
    transfer_response_timeout: changed(
      from.transfer_response_timeout,
      to.transfer_response_timeout,
    ),
    deduplicate_search_requests: changed(
      from.deduplicate_search_requests,
      to.deduplicate_search_requests,
    ),
    auto_acknowledge_private_messages: changed(
      from.auto_acknowledge_private_messages,
      to.auto_acknowledge_private_messages,
    ),
    auto_acknowledge_privilege_notifications: changed(
      from.auto_acknowledge_privilege_notifications,
      to.auto_acknowledge_privilege_notifications,
    ),
    accept_private_room_invitations: changed(
      from.accept_private_room_invitations,
      to.accept_private_room_invitations,
    ),
    server_connection_options: changed(
      from.server_connection_options,
      to.server_connection_options,
    ),
    peer_connection_options: changed(from.peer_connection_options, to.peer_connection_options),
    transfer_connection_options: changed(
      from.transfer_connection_options,
      to.transfer_connection_options,
    ),
    incoming_connection_options: changed(
      from.incoming_connection_options,
      to.incoming_connection_options,
    ),
    distributed_connection_options: changed(
      from.distributed_connection_options,
      to.distributed_connection_options,
    ),
    server_keepalive: changed(from.server_keepalive, to.server_keepalive),
    report_unread_message_data: changed(
      from.report_unread_message_data,
      to.report_unread_message_data,
    ),
    limits: changed(from.limits, to.limits),
    resolvers: soulseek_client::Resolvers::default(),
  })
}

/// How every download asks the peer for its file under `options`.
pub fn request_style(options: &Options) -> RequestStyle {
  match options.transfers.download.request_style {
    enums::RequestStyle::Queue => RequestStyle::Queue,
    enums::RequestStyle::TransferRequest => RequestStyle::TransferRequest,
  }
}

/// The options of each kind of connection.
struct ConnectionSets {
  server: ConnectionOptions,
  peer: ConnectionOptions,
  transfer: ConnectionOptions,
  distributed: ConnectionOptions,
}

impl ConnectionSets {
  /// Peer connections take the base options; the server connection never times out for
  /// inactivity (the client's keep-alive notices a dead one) and has the MTU's segment size;
  /// transfers have their own buffers and timeout; distributed connections have the configured
  /// write queue and a keep-alive from the inactivity timeout, since a parent may be quiet.
  fn of(soulseek: &Soulseek) -> Result<Self, String> {
    let connection = &soulseek.connection;
    let timeout = &connection.timeout;
    let buffer = &connection.buffer;
    let base = ConnectionOptions {
      read_buffer_size: buffer.read as usize,
      write_buffer_size: buffer.write as usize,
      write_queue_size: UNBOUNDED_WRITE_QUEUE,
      connect_timeout: timeout.connect,
      inactivity_timeout: Some(timeout.inactivity),
      proxy: proxy(soulseek)?,
      keepalive: KeepAlive::default(),
      max_segment_size: None,
    };
    // keep-alive takes whole seconds
    let seconds = Duration::from_secs(timeout.inactivity.as_secs());
    Ok(Self {
      server: ConnectionOptions {
        inactivity_timeout: None,
        max_segment_size: connection.mtu.map(|mtu| mtu - HEADERS),
        ..base.clone()
      },
      transfer: ConnectionOptions {
        read_buffer_size: buffer.transfer as usize,
        write_buffer_size: buffer.transfer as usize,
        inactivity_timeout: Some(timeout.transfer),
        ..base.clone()
      },
      distributed: ConnectionOptions {
        write_queue_size: buffer.write_queue,
        keepalive: KeepAlive {
          enabled: true,
          idle: seconds,
          interval: seconds,
          probes: DISTRIBUTED_PROBES,
        },
        ..base.clone()
      },
      peer: base,
    })
  }
}

/// The SOCKS5 proxy every outbound connection goes through, when it is on.
fn proxy(soulseek: &Soulseek) -> Result<Option<ProxyOptions>, String> {
  let proxy = &soulseek.connection.proxy;
  if !proxy.enabled {
    return Ok(None);
  }
  let (Some(address), Some(port)) = (&proxy.address, proxy.port) else {
    return Err("soulseek.connection.proxy: enabled without an address and a port".into());
  };
  let credentials = match (&proxy.username, &proxy.password) {
    (Some(username), Some(password)) => Some(ProxyCredentials {
      username: username.clone(),
      password: Secret::new(password.as_str()),
    }),
    (None, None) => None,
    _ => {
      return Err("soulseek.connection.proxy: a username without a password or the reverse".into());
    }
  };
  Ok(Some(ProxyOptions {
    address: address.clone(),
    port,
    credentials,
  }))
}

#[cfg(test)]
mod tests {
  use super::*;
  use soulseek_client::Client;

  fn options(toml: &str) -> Options {
    toml::from_str(toml).unwrap()
  }

  #[test]
  fn connection_sets_follow_the_kind_of_connection() {
    let client = client_options(&options(
      "[soulseek.connection]\nmtu = 1500\n[soulseek.connection.buffer]\ntransfer = 100000\n\
       write_queue = 75\n[soulseek.connection.timeout]\ninactivity = \"20s\"\ntransfer = \"45s\"\n",
    ))
    .unwrap();
    let server = &client.server_connection_options;
    assert_eq!(server.inactivity_timeout, None);
    assert_eq!(server.max_segment_size, Some(1460));
    assert_eq!(server.write_queue_size, UNBOUNDED_WRITE_QUEUE);
    let peer = &client.peer_connection_options;
    assert_eq!(peer.write_queue_size, UNBOUNDED_WRITE_QUEUE);
    assert_eq!(peer.inactivity_timeout, Some(Duration::from_secs(20)));
    assert_eq!(peer.max_segment_size, None);
    assert_eq!(&client.incoming_connection_options, peer);
    let transfer = &client.transfer_connection_options;
    assert_eq!(
      (transfer.read_buffer_size, transfer.write_buffer_size),
      (100_000, 100_000)
    );
    assert_eq!(transfer.inactivity_timeout, Some(Duration::from_secs(45)));
    assert_eq!(transfer.write_queue_size, UNBOUNDED_WRITE_QUEUE);
    let distributed = &client.distributed_connection_options;
    assert_eq!(distributed.write_queue_size, 75);
    assert_eq!(
      distributed.keepalive,
      KeepAlive {
        enabled: true,
        idle: Duration::from_secs(20),
        interval: Duration::from_secs(20),
        probes: 3
      }
    );
    client.validate().unwrap();
  }

  #[test]
  fn unset_limits_are_none_for_the_client() {
    let client = client_options(&Options::default()).unwrap();
    assert_eq!(
      (client.maximum_upload_speed, client.maximum_download_speed),
      (0, 0)
    );
    assert_eq!(client.maximum_concurrent_uploads, 10);
    assert_eq!(client.maximum_concurrent_downloads, i32::MAX as u32);
    let limited = client_options(&options(
      "[transfers.upload]\nspeed_limit = 100\nslots = 3\n[transfers.download]\nslots = 4\n",
    ))
    .unwrap();
    assert_eq!(limited.maximum_upload_speed, 100);
    assert_eq!(limited.maximum_concurrent_uploads, 3);
    assert_eq!(limited.maximum_concurrent_downloads, 4);
  }

  /// A client started with the options of `previous` and patched to `current` runs with the
  /// options a client started with `current` has.
  async fn patched_like_started(previous: &Options, current: &Options) {
    let client = Client::new(client_options(previous).unwrap()).unwrap();
    let reconnect = client
      .reconfigure_options(patch(previous, current).unwrap())
      .await
      .unwrap();
    assert!(!reconnect, "a disconnected client never needs a reconnect");
    assert_eq!(
      format!("{:?}", client.options()),
      format!("{:?}", client_options(current).unwrap())
    );
    client.dispose().await;
  }

  #[tokio::test]
  async fn the_patch_gives_the_options_a_start_would() {
    let previous = Options::default();
    for current in [
      "[soulseek.connection.buffer]\ntransfer = 90000\nwrite_queue = 9\n",
      "[soulseek.connection.timeout]\ninactivity = \"30s\"\nconnect = \"3s\"\n",
      "[soulseek.connection]\nmtu = 1400\n",
      "[soulseek.connection.proxy]\nenabled = true\naddress = \"proxy\"\nport = 1080\n\
       username = \"u\"\npassword = \"p\"\n",
      "[soulseek.distributed_network]\ndisabled = true\nchild_limit = 5\n",
      "[transfers.upload]\nslots = 2\nspeed_limit = 50\n[transfers.download]\nslots = 7\n",
      "[soulseek.server_keepalive]\nprobes = 9\n[soulseek.limits]\nmax_cached_peer_connections = 7\n",
      "[soulseek]\nlisten_port = 50301\n",
    ] {
      patched_like_started(&previous, &options(current)).await;
      patched_like_started(&options(current), &previous).await;
    }
  }

  #[test]
  fn only_what_changed_is_patched_but_the_speeds_always_are() {
    let previous = Options::default();
    let patch = patch(&previous, &previous).unwrap();
    assert_eq!(patch.maximum_upload_speed, Some(0));
    assert_eq!(patch.maximum_download_speed, Some(0));
    assert!(patch.listen_port.is_none() && patch.server_connection_options.is_none());
    let current = options("[soulseek.connection.timeout]\ntransfer = \"40s\"\n");
    let patch = super::patch(&previous, &current).unwrap();
    assert!(patch.transfer_connection_options.is_some());
    assert!(
      patch.server_connection_options.is_none(),
      "the server's own options did not change"
    );
  }
}
