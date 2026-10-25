//! Who is on the other end of a request: the client's address and the host it asked for, after
//! the headers of trusted reverse proxies.

use std::net::{IpAddr, Ipv4Addr};

use axum::http::{HeaderMap, Uri, header};

use crate::config::cidr::{IpRange, unmapped};
use crate::http::{Peer, Transport};

/// The client of a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Client {
  /// the client's address; a request over the unix socket comes from `127.0.0.1`
  pub ip: IpAddr,
  /// the host the request was sent to, without its port
  pub host: Option<String>,
  /// whether the client spoke HTTPS, to us or to the proxy in front
  pub https: bool,
}

/// The client of a request that reached us from `peer`. `X-Forwarded-For`,
/// `X-Forwarded-Proto` and `X-Forwarded-Host` are believed only when the peer is one of
/// `proxies`; the client is then the rightmost address of `X-Forwarded-For` that is not a
/// trusted proxy.
pub fn resolve(peer: &Peer, uri: &Uri, headers: &HeaderMap, proxies: &[IpRange]) -> Client {
  let peer_ip = peer
    .remote
    .map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |addr| unmapped(addr.ip()));
  let host_header = headers
    .get(header::HOST)
    .and_then(|h| h.to_str().ok())
    .map(str::to_string)
    .or_else(|| uri.authority().map(|a| a.to_string()));
  let direct = Client {
    ip: peer_ip,
    host: host_header.as_deref().map(without_port),
    https: peer.transport == Transport::Tls,
  };
  let trusted = |ip: IpAddr| proxies.iter().any(|range| range.contains(ip));
  if !trusted(peer_ip) {
    return direct;
  }
  let forwarded = |name: &str| {
    headers
      .get(name)
      .and_then(|h| h.to_str().ok())
      .map(str::trim)
      .filter(|v| !v.is_empty())
  };
  let hops: Vec<IpAddr> = forwarded("x-forwarded-for")
    .map(|list| {
      list
        .split(',')
        .filter_map(|hop| hop.trim().parse().ok())
        .map(unmapped)
        .collect()
    })
    .unwrap_or_default();
  let ip = hops
    .iter()
    .rev()
    .find(|ip| !trusted(**ip))
    .or(hops.first())
    .copied()
    .unwrap_or(peer_ip);
  let https = match forwarded("x-forwarded-proto") {
    Some(proto) => proto.eq_ignore_ascii_case("https"),
    None => direct.https,
  };
  let host = forwarded("x-forwarded-host")
    .map(without_port)
    .or(direct.host);
  Client { ip, host, https }
}

/// `host` without a `:port`, and without the brackets of an IPv6 address.
pub fn without_port(host: &str) -> String {
  if host.parse::<IpAddr>().is_ok() {
    return host.to_string();
  }
  if let Some(rest) = host.strip_prefix('[') {
    return rest.split(']').next().unwrap_or(rest).to_string();
  }
  match host.rsplit_once(':') {
    Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name.to_string(),
    _ => host.to_string(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use axum::http::HeaderValue;
  use std::net::SocketAddr;

  fn peer(addr: &str) -> Peer {
    Peer {
      remote: Some(addr.parse::<SocketAddr>().unwrap()),
      transport: Transport::Plain,
    }
  }

  fn headers(items: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in items {
      map.insert(*name, HeaderValue::from_static(value));
    }
    map
  }

  #[test]
  fn forwarded_headers_count_only_from_trusted_proxies() {
    let proxies = vec![
      "127.0.0.1/32".parse().unwrap(),
      "10.0.0.0/8".parse().unwrap(),
    ];
    let h = headers(&[
      ("host", "127.0.0.1:5030"),
      ("x-forwarded-for", "203.0.113.9, 198.51.100.1, 10.0.0.2"),
      ("x-forwarded-proto", "https"),
      ("x-forwarded-host", "reseek.example.net"),
    ]);
    let uri = Uri::from_static("/api/v0/options");
    let via_proxy = resolve(&peer("127.0.0.1:40000"), &uri, &h, &proxies);
    assert_eq!(via_proxy.ip, "198.51.100.1".parse::<IpAddr>().unwrap());
    assert_eq!(via_proxy.host.as_deref(), Some("reseek.example.net"));
    assert!(via_proxy.https);
    let direct = resolve(&peer("192.0.2.7:40000"), &uri, &h, &proxies);
    assert_eq!(direct.ip, "192.0.2.7".parse::<IpAddr>().unwrap());
    assert_eq!(direct.host.as_deref(), Some("127.0.0.1"));
    assert!(!direct.https);
  }

  #[test]
  fn the_unix_socket_is_local_and_hosts_lose_their_port() {
    let unix = Peer {
      remote: None,
      transport: Transport::Unix,
    };
    let client = resolve(
      &unix,
      &Uri::from_static("/"),
      &headers(&[("host", "[::1]:5030")]),
      &[],
    );
    assert_eq!(client.ip, IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_eq!(client.host.as_deref(), Some("::1"));
    assert_eq!(without_port("localhost:5030"), "localhost");
    assert_eq!(without_port("example.net"), "example.net");
    assert_eq!(without_port("[::1]"), "::1");
    assert_eq!(without_port("[fe80::1]:5030"), "fe80::1");
    assert_eq!(without_port("::1"), "::1");
    assert_eq!(without_port("192.0.2.1:80"), "192.0.2.1");
  }
}
