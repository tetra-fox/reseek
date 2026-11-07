//! The Soulseek options: ports, the listen address, the picture, the interests, timeouts,
//! buffers, the proxy and the server MTU.

use std::time::Duration;

use super::Report;
use super::fields::{at_least, duration_at_least, ip_address, length, range};
use super::paths::readable_file;
use crate::config::{Soulseek, SoulseekInterests};

pub(super) fn check(soulseek: &Soulseek, report: &mut Report) {
  for (key, port) in [
    ("soulseek.port", soulseek.port),
    ("soulseek.listen_port", soulseek.listen_port),
  ] {
    report.check(key, range(key, port, 1024, 65535));
  }
  report.check(
    "soulseek.listen_ip_address",
    ip_address("soulseek.listen_ip_address", &soulseek.listen_ip_address),
  );
  if let Some(picture) = &soulseek.picture {
    report.check(
      "soulseek.picture",
      readable_file("soulseek.picture", picture),
    );
  }
  check_interests(&soulseek.interests, report);
  let key = "soulseek.search.max_concurrent";
  report.check(key, at_least(key, soulseek.search.max_concurrent, 1));
  let key = "soulseek.browse.response_timeout";
  report.check(
    key,
    duration_at_least(
      key,
      soulseek.browse.response_timeout,
      Duration::from_secs(1),
    ),
  );
  let key = "soulseek.distributed_network.child_limit";
  report.check(
    key,
    at_least(key, soulseek.distributed_network.child_limit, 1),
  );

  let connection = &soulseek.connection;
  let timeout = &connection.timeout;
  for (key, value, min) in [
    ("soulseek.connection.timeout.connect", timeout.connect, 1),
    (
      "soulseek.connection.timeout.inactivity",
      timeout.inactivity,
      1,
    ),
    ("soulseek.connection.timeout.transfer", timeout.transfer, 30),
    (
      "soulseek.connection.timeout.transfer_response",
      timeout.transfer_response,
      1,
    ),
  ] {
    report.check(key, duration_at_least(key, value, Duration::from_secs(min)));
  }
  let buffer = &connection.buffer;
  for (key, value, min) in [
    ("soulseek.connection.buffer.read", buffer.read, 1024),
    ("soulseek.connection.buffer.write", buffer.write, 1024),
    (
      "soulseek.connection.buffer.transfer",
      buffer.transfer,
      81920,
    ),
  ] {
    report.check(key, at_least(key, value, min));
  }
  let key = "soulseek.connection.buffer.write_queue";
  report.check(key, range(key, buffer.write_queue, 5, 5000));

  let proxy = &connection.proxy;
  if proxy.enabled {
    if proxy.address.is_none() {
      report.add(
        "soulseek.connection.proxy.address",
        "The field soulseek.connection.proxy.enabled is true, but no \
         soulseek.connection.proxy.address has been specified.",
      );
    }
    if proxy.port.is_none() {
      report.add(
        "soulseek.connection.proxy.port",
        "The field soulseek.connection.proxy.enabled is true, but no \
         soulseek.connection.proxy.port has been specified.",
      );
    }
  }
  for (key, value) in [
    ("soulseek.connection.proxy.address", &proxy.address),
    ("soulseek.connection.proxy.username", &proxy.username),
    ("soulseek.connection.proxy.password", &proxy.password),
  ] {
    report.check(key, length(key, value.as_deref(), 1, 255));
  }
  if proxy.username.is_some() != proxy.password.is_some() {
    report.add(
      "soulseek.connection.proxy.username",
      "The fields soulseek.connection.proxy.username and soulseek.connection.proxy.password \
       must be set together.",
    );
  }
  if let Some(port) = proxy.port {
    let key = "soulseek.connection.proxy.port";
    report.check(key, range(key, port, 1, 65535));
  }
  if let Some(mtu) = connection.mtu {
    let key = "soulseek.connection.mtu";
    report.check(key, range(key, mtu, 576, 65535));
  }
  check_keepalive(soulseek, report);
  check_limits(soulseek, report);
}

/// Each interest as the server is sent it, once: not blank, trimmed and lowercase, as other
/// clients keep theirs, and in one of the two lists at most.
fn check_interests(interests: &SoulseekInterests, report: &mut Report) {
  for (key, list) in [
    ("soulseek.interests.liked", &interests.liked),
    ("soulseek.interests.hated", &interests.hated),
  ] {
    for (at, interest) in list.iter().enumerate() {
      if interest.trim().is_empty() {
        report.add(
          key,
          format!("The field {key} must not hold a blank interest."),
        );
      } else if interest.trim() != interest || interest.to_lowercase() != *interest {
        report.add(
          key,
          format!(
            "The interest '{interest}' in {key} must be lowercase, without spaces around it."
          ),
        );
      } else if list[..at].contains(interest) {
        report.add(
          key,
          format!("The interest '{interest}' is listed twice in {key}."),
        );
      }
    }
  }
  for interest in interests
    .liked
    .iter()
    .filter(|i| interests.hated.contains(i))
  {
    report.add(
      "soulseek.interests.hated",
      format!(
        "The interest '{interest}' is in both soulseek.interests.liked and \
         soulseek.interests.hated."
      ),
    );
  }
}

fn check_keepalive(soulseek: &Soulseek, report: &mut Report) {
  let keepalive = &soulseek.server_keepalive;
  // the other fields mean nothing while it is off
  if !keepalive.enabled {
    return;
  }
  for (key, value) in [
    ("soulseek.server_keepalive.idle", keepalive.idle),
    ("soulseek.server_keepalive.interval", keepalive.interval),
  ] {
    report.check(key, duration_at_least(key, value, Duration::from_secs(1)));
  }
  let key = "soulseek.server_keepalive.probes";
  report.check(key, at_least(key, keepalive.probes, 1));
}

fn check_limits(soulseek: &Soulseek, report: &mut Report) {
  let limits = &soulseek.limits;
  let key = "soulseek.limits.max_message_size";
  // a message must at least hold its 4-byte code, and the client counts it in 32 bits
  if !(4..=u64::from(u32::MAX)).contains(&limits.max_message_size.as_u64()) {
    report.add(
      key,
      format!(
        "The field {key} must be between 4 bytes and {}.",
        bytesize::ByteSize(u64::from(u32::MAX))
      ),
    );
  }
  for (key, value) in [
    (
      "soulseek.limits.max_concurrent_peer_connects",
      limits.max_concurrent_peer_connects,
    ),
    (
      "soulseek.limits.max_cached_peer_connections",
      limits.max_cached_peer_connections,
    ),
    (
      "soulseek.limits.search_response_cache_size",
      limits.search_response_cache_size,
    ),
    (
      "soulseek.limits.user_endpoint_cache_size",
      limits.user_endpoint_cache_size,
    ),
  ] {
    report.check(key, at_least(key, value, 1));
  }
  let key = "soulseek.limits.distributed_parent_negotiation_timeout";
  report.check(
    key,
    duration_at_least(
      key,
      limits.distributed_parent_negotiation_timeout,
      Duration::from_millis(1),
    ),
  );
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn interests_are_lowercase_once_and_in_one_list() {
    let owned = |list: &[&str]| list.iter().map(ToString::to_string).collect();
    let interests = SoulseekInterests {
      liked: owned(&["jazz", "Dub", " folk", "", "jazz", "noise"]),
      hated: owned(&["polka", "noise"]),
    };
    let mut report = Report::default();
    check_interests(&interests, &mut report);
    assert_eq!(
      report.messages(),
      [
        "The interest 'Dub' in soulseek.interests.liked must be lowercase, without spaces around \
         it.",
        "The interest ' folk' in soulseek.interests.liked must be lowercase, without spaces \
         around it.",
        "The field soulseek.interests.liked must not hold a blank interest.",
        "The interest 'jazz' is listed twice in soulseek.interests.liked.",
        "The interest 'noise' is in both soulseek.interests.liked and soulseek.interests.hated.",
      ]
    );
  }
}
