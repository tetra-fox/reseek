//! Trusted networks: clients whose address needs no credential, as long as they reach the
//! daemon by a name that a page on another site could not point at it.

use api_types::auth::{Principal, Via};

use super::client::Client;
use crate::config::Options;
use crate::config::cidr::{ANY, IpRange, parse_list};

/// The principal of a client from a trusted network, `None` when the client's address is in
/// no trusted network or it reached the daemon by a host that is not allowed. With
/// `authentication_off`, the value the daemon started with, every network is trusted.
pub fn trusted(client: &Client, options: &Options, authentication_off: bool) -> Option<Principal> {
  let authentication = &options.web.authentication;
  let mut networks: Vec<String> = authentication.trusted_networks.clone();
  if authentication_off {
    networks.extend(ANY.split(',').map(str::to_string));
  }
  let network = networks.iter().find(|text| match text.parse::<IpRange>() {
    Ok(range) => range.contains(client.ip),
    Err(e) => {
      // checked when the configuration loaded, so this is a bug if it happens
      tracing::error!(network = text.as_str(), error = %e, "trusted network does not parse");
      false
    }
  })?;
  if !host_allowed(client.host.as_deref(), &options.web.hostnames) {
    tracing::debug!(
      ip = %client.ip,
      host = client.host.as_deref(),
      "trusted network not trusted through this host"
    );
    return None;
  }
  Some(Principal {
    subject: format!("network:{network}"),
    name: "trusted network".to_string(),
    role: authentication.trusted_role.into(),
    via: Via::TrustedNetwork {
      cidr: network.clone(),
    },
  })
}

/// Whether a trusted network may be trusted through `host`: an IP address, `localhost`, or one
/// of `hostnames`. A request that names no host is not.
pub fn host_allowed(host: Option<&str>, hostnames: &[String]) -> bool {
  let Some(host) = host else {
    return false;
  };
  host.parse::<std::net::IpAddr>().is_ok()
    || host.eq_ignore_ascii_case("localhost")
    || hostnames.iter().any(|name| name.eq_ignore_ascii_case(host))
}

/// The networks a list of ranges in the configuration stands for, skipping any that do not
/// parse, which loading the configuration already refused.
pub fn ranges(texts: &[String]) -> Vec<IpRange> {
  texts
    .iter()
    .filter_map(|text| match parse_list(text) {
      Ok(ranges) => Some(ranges),
      Err((element, e)) => {
        tracing::error!(element, error = %e, "network does not parse");
        None
      }
    })
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn client(ip: &str, host: Option<&str>) -> Client {
    Client {
      ip: ip.parse().unwrap(),
      host: host.map(str::to_string),
      authority: host.map(str::to_string),
      https: false,
    }
  }

  #[test]
  fn a_trusted_network_is_trusted_only_through_safe_hosts() {
    let mut options = Options::default();
    options.web.authentication.trusted_networks = vec!["192.168.1.0/24".into()];
    options.web.hostnames = vec!["reseek.lan".into()];
    let lan = |host| trusted(&client("192.168.1.5", host), &options, false);
    assert_eq!(
      lan(Some("192.168.1.2")).unwrap().subject,
      "network:192.168.1.0/24"
    );
    assert!(lan(Some("localhost")).is_some());
    assert!(lan(Some("Reseek.LAN")).is_some());
    assert!(lan(Some("evil.example")).is_none(), "the rebinding rule");
    assert!(lan(None).is_none());
    assert!(trusted(&client("10.0.0.5", Some("localhost")), &options, false).is_none());
  }

  #[test]
  fn turning_authentication_off_trusts_every_network_with_the_same_host_rule() {
    let options = Options::default();
    let off = |ip, host| trusted(&client(ip, Some(host)), &options, true);
    let anywhere = off("203.0.113.1", "203.0.113.2").unwrap();
    assert_eq!(anywhere.role, api_types::auth::Role::Administrator);
    assert!(off("2001:db8::1", "localhost").is_some());
    assert!(off("203.0.113.1", "evil.example").is_none());
  }
}
