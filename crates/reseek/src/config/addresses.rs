//! IP addresses as the listener options write them: one address, IPv6 with or without
//! brackets, or a comma-separated list of them.

use std::net::{IpAddr, Ipv6Addr};

/// The address `text` names, IPv6 with or without brackets (`[::1]`).
pub fn address(text: &str) -> Option<IpAddr> {
  let bare = text
    .strip_prefix('[')
    .and_then(|t| t.strip_suffix(']'))
    .unwrap_or(text);
  bare.parse().ok()
}

/// The addresses of a comma-separated list, each trimmed; every address (`[::]`, which takes
/// IPv4 too) when the list is not set. The error is the first element that is no address, so
/// a blank list is refused.
pub fn list(text: Option<&str>) -> Result<Vec<IpAddr>, String> {
  let Some(text) = text else {
    return Ok(vec![IpAddr::V6(Ipv6Addr::UNSPECIFIED)]);
  };
  text
    .split(',')
    .map(|item| address(item.trim()).ok_or_else(|| item.trim().to_string()))
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn lists_are_trimmed_bracketed_or_not_and_unset_is_every_address() {
    assert_eq!(list(None).unwrap(), [IpAddr::V6(Ipv6Addr::UNSPECIFIED)]);
    assert_eq!(list(Some("  ")).unwrap_err(), "");
    assert_eq!(list(Some("")).unwrap_err(), "");
    assert_eq!(
      list(Some("127.0.0.1, [::1] ,::")).unwrap(),
      [
        "127.0.0.1".parse::<IpAddr>().unwrap(),
        "::1".parse().unwrap(),
        "::".parse().unwrap()
      ]
    );
    assert_eq!(list(Some("127.0.0.1,nope")).unwrap_err(), "nope");
    assert_eq!(list(Some("localhost")).unwrap_err(), "localhost");
    assert_eq!(address("[fe80::1]"), "fe80::1".parse().ok());
  }
}
