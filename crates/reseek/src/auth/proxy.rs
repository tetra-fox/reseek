//! The identity a trusted reverse proxy forwards in headers, such as an Authentik outpost's
//! `X-Authentik-Username`. The headers count only when the connection itself comes from a
//! trusted proxy; from anyone else they are ignored.

use std::net::IpAddr;

use api_types::auth::Role;
use axum::http::HeaderMap;

use super::roles::Mapping;
use crate::config::Proxy;
use crate::config::cidr::IpRange;

/// The identity a proxy forwarded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forwarded {
  /// the username
  pub username: String,
  /// the user's groups
  pub groups: Vec<String>,
  /// the user's email address
  pub email: Option<String>,
}

impl Forwarded {
  /// The subject its requests act as.
  pub fn subject(&self) -> String {
    format!("proxy:{}", self.username)
  }

  /// The role its groups give, `None` when it is refused.
  pub fn role(&self, proxy: &Proxy) -> Option<Role> {
    Mapping::from(&proxy.role_mapping).role(&self.groups)
  }
}

/// The identity in `headers`, when forwarded identities are on, `peer` (the connection's own
/// address, before any forwarding) is one of `proxies`, and a username is given.
pub fn forwarded(
  headers: &HeaderMap,
  peer: IpAddr,
  proxy: &Proxy,
  proxies: &[IpRange],
) -> Option<Forwarded> {
  if !proxy.enabled || !proxies.iter().any(|range| range.contains(peer)) {
    return None;
  }
  let header = |name: &str| {
    headers
      .get(name)
      .and_then(|value| value.to_str().ok())
      .map(str::trim)
      .filter(|value| !value.is_empty())
  };
  let username = header(&proxy.user_header)?.to_string();
  let groups = header(&proxy.groups_header)
    .map(|list| {
      list
        .split(['|', ','])
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .map(str::to_string)
        .collect()
    })
    .unwrap_or_default();
  Some(Forwarded {
    username,
    groups,
    email: header(&proxy.email_header).map(str::to_string),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::enums::DefaultRole;

  #[test]
  fn forwarded_identities_count_only_from_trusted_proxies() {
    let mut proxy = Proxy {
      enabled: true,
      ..Proxy::default()
    };
    proxy.role_mapping.readwrite = vec!["users".into()];
    proxy.role_mapping.default = DefaultRole::Deny;
    let proxies = vec!["127.0.0.1/32".parse().unwrap()];
    let mut headers = HeaderMap::new();
    headers.insert("x-authentik-username", "alice".parse().unwrap());
    headers.insert("x-authentik-groups", "admins|users, staff".parse().unwrap());
    headers.insert("x-authentik-email", "alice@example.net".parse().unwrap());
    let local: IpAddr = "127.0.0.1".parse().unwrap();
    let found = forwarded(&headers, local, &proxy, &proxies).unwrap();
    assert_eq!(found.groups, ["admins", "users", "staff"]);
    assert_eq!(found.subject(), "proxy:alice");
    assert_eq!(found.role(&proxy), Some(Role::ReadWrite));
    assert!(forwarded(&headers, "192.0.2.1".parse().unwrap(), &proxy, &proxies).is_none());
    proxy.enabled = false;
    assert!(forwarded(&headers, local, &proxy, &proxies).is_none());
  }
}
