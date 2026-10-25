//! The Soulseek options: ports, the listen address, the picture, timeouts, buffers, the proxy
//! and the server MTU.

use std::time::Duration;

use super::Report;
use super::fields::{at_least, duration_at_least, ip_address, length, range};
use super::paths::readable_file;
use crate::config::Soulseek;

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
  if let Some(port) = proxy.port {
    let key = "soulseek.connection.proxy.port";
    report.check(key, range(key, port, 1, 65535));
  }
  if let Some(mtu) = connection.mtu {
    let key = "soulseek.connection.mtu";
    report.check(key, range(key, mtu, 576, 65535));
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_enabled_proxy_needs_its_address_and_port() {
    let mut soulseek = Soulseek::default();
    soulseek.connection.proxy.enabled = true;
    let mut report = Report::default();
    check(&soulseek, &mut report);
    let keys: Vec<&str> = report.0.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(
      keys,
      [
        "soulseek.connection.proxy.address",
        "soulseek.connection.proxy.port"
      ]
    );
  }
}
