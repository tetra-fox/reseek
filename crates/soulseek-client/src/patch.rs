//! A partial change to the client options and what applying it implies: whether the listener
//! must be rebound and whether the change only takes effect after reconnecting.

use std::net::IpAddr;

use crate::error::ClientError;
use crate::options::{ClientLimits, ClientOptions, ConnectionOptions, KeepAlive};
use crate::resolvers::Resolvers;

/// The options that can change while the client runs. Each field replaces the [`ClientOptions`]
/// field of the same name, which says what it does; `None` leaves it as it is. The options fixed
/// for the client's lifetime (concurrency limits, message timeout, starting token, protocol
/// versions) have no field here.
#[derive(Clone, Debug, Default)]
pub struct ClientOptionsPatch {
  pub enable_listener: Option<bool>,
  pub listen_ip_address: Option<IpAddr>,
  pub listen_port: Option<u16>,
  pub listen_backlog: Option<u32>,
  pub enable_distributed_network: Option<bool>,
  pub accept_distributed_children: Option<bool>,
  pub distributed_child_limit: Option<u32>,
  pub maximum_upload_speed: Option<u32>,
  pub maximum_download_speed: Option<u32>,
  pub deduplicate_search_requests: Option<bool>,
  pub auto_acknowledge_private_messages: Option<bool>,
  pub auto_acknowledge_privilege_notifications: Option<bool>,
  pub accept_private_room_invitations: Option<bool>,
  /// its inactivity timeout is turned off, as for the options the client starts with
  pub server_connection_options: Option<ConnectionOptions>,
  pub peer_connection_options: Option<ConnectionOptions>,
  pub transfer_connection_options: Option<ConnectionOptions>,
  pub incoming_connection_options: Option<ConnectionOptions>,
  pub distributed_connection_options: Option<ConnectionOptions>,
  pub server_keepalive: Option<KeepAlive>,
  pub report_unread_message_data: Option<bool>,
  pub limits: Option<ClientLimits>,
  /// each callback that is set replaces the current one
  pub resolvers: Resolvers,
}

impl ClientOptionsPatch {
  /// `current` with every set field replaced, validated as a whole
  pub(crate) fn apply(self, current: &ClientOptions) -> Result<ClientOptions, ClientError> {
    let Self {
      enable_listener,
      listen_ip_address,
      listen_port,
      listen_backlog,
      enable_distributed_network,
      accept_distributed_children,
      distributed_child_limit,
      maximum_upload_speed,
      maximum_download_speed,
      deduplicate_search_requests,
      auto_acknowledge_private_messages,
      auto_acknowledge_privilege_notifications,
      accept_private_room_invitations,
      server_connection_options,
      peer_connection_options,
      transfer_connection_options,
      incoming_connection_options,
      distributed_connection_options,
      server_keepalive,
      report_unread_message_data,
      limits,
      resolvers,
    } = self;
    let mut next = current.clone();
    set(&mut next.enable_listener, enable_listener);
    set(&mut next.listen_ip_address, listen_ip_address);
    set(&mut next.listen_port, listen_port);
    set(&mut next.listen_backlog, listen_backlog);
    set(
      &mut next.enable_distributed_network,
      enable_distributed_network,
    );
    set(
      &mut next.accept_distributed_children,
      accept_distributed_children,
    );
    set(&mut next.distributed_child_limit, distributed_child_limit);
    set(&mut next.maximum_upload_speed, maximum_upload_speed);
    set(&mut next.maximum_download_speed, maximum_download_speed);
    set(
      &mut next.deduplicate_search_requests,
      deduplicate_search_requests,
    );
    set(
      &mut next.auto_acknowledge_private_messages,
      auto_acknowledge_private_messages,
    );
    set(
      &mut next.auto_acknowledge_privilege_notifications,
      auto_acknowledge_privilege_notifications,
    );
    set(
      &mut next.accept_private_room_invitations,
      accept_private_room_invitations,
    );
    if let Some(options) = server_connection_options {
      next.server_connection_options = ConnectionOptions {
        inactivity_timeout: None,
        ..options
      };
    }
    set(&mut next.peer_connection_options, peer_connection_options);
    set(
      &mut next.transfer_connection_options,
      transfer_connection_options,
    );
    set(
      &mut next.incoming_connection_options,
      incoming_connection_options,
    );
    set(
      &mut next.distributed_connection_options,
      distributed_connection_options,
    );
    set(&mut next.server_keepalive, server_keepalive);
    set(
      &mut next.report_unread_message_data,
      report_unread_message_data,
    );
    set(&mut next.limits, limits);
    next.resolvers.merge(resolvers);
    next.validate()?;
    Ok(next)
  }
}

fn set<T>(field: &mut T, value: Option<T>) {
  if let Some(value) = value {
    *field = value;
  }
}

/// the listener's socket has to be bound again for the change to take effect
pub(crate) fn listener_changed(old: &ClientOptions, new: &ClientOptions) -> bool {
  old.enable_listener != new.enable_listener
    || old.listen_ip_address != new.listen_ip_address
    || old.listen_port != new.listen_port
    || old.listen_backlog != new.listen_backlog
}

/// The change only takes effect on a new server session: the distributed network or children
/// being turned off (the server and the tree already count on us), or the server or distributed
/// connections' options changing. Turning things on, and the peer, transfer and incoming
/// options, apply to what happens next without reconnecting.
pub(crate) fn reconnect_required(old: &ClientOptions, new: &ClientOptions) -> bool {
  (old.enable_distributed_network && !new.enable_distributed_network)
    || (old.accept_distributed_children && !new.accept_distributed_children)
    || old.distributed_connection_options != new.distributed_connection_options
    || old.effective_server_connection_options() != new.effective_server_connection_options()
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;

  #[test]
  fn reconnect_required_only_for_turning_things_off_or_server_and_distributed_options() {
    let old = ClientOptions::default();
    let patched = |patch: ClientOptionsPatch| patch.apply(&old).unwrap();

    let off = patched(ClientOptionsPatch {
      enable_distributed_network: Some(false),
      ..ClientOptionsPatch::default()
    });
    assert!(reconnect_required(&old, &off));
    assert!(!reconnect_required(&off, &old), "turning it back on");

    let no_children = patched(ClientOptionsPatch {
      accept_distributed_children: Some(false),
      ..ClientOptionsPatch::default()
    });
    assert!(reconnect_required(&old, &no_children));

    let peer = patched(ClientOptionsPatch {
      peer_connection_options: Some(ConnectionOptions {
        connect_timeout: Duration::from_secs(3),
        ..ConnectionOptions::default()
      }),
      ..ClientOptionsPatch::default()
    });
    assert!(!reconnect_required(&old, &peer));

    let keepalive = patched(ClientOptionsPatch {
      server_keepalive: Some(KeepAlive {
        probes: 5,
        ..old.server_keepalive.clone()
      }),
      ..ClientOptionsPatch::default()
    });
    assert!(reconnect_required(&old, &keepalive));
  }

  #[test]
  fn server_options_from_a_patch_never_time_out() {
    let old = ClientOptions::default();
    let new = ClientOptionsPatch {
      server_connection_options: Some(ConnectionOptions::default()),
      ..ClientOptionsPatch::default()
    }
    .apply(&old)
    .unwrap();
    assert_eq!(new.server_connection_options.inactivity_timeout, None);
    assert!(
      !reconnect_required(&old, &new),
      "identical once the timeout is forced off"
    );
  }

  #[test]
  fn invalid_patch_is_rejected_whole() {
    let old = ClientOptions::default();
    let result = ClientOptionsPatch {
      listen_port: Some(80),
      ..ClientOptionsPatch::default()
    }
    .apply(&old);
    assert!(matches!(result, Err(ClientError::InvalidArgument(_))));
  }
}
