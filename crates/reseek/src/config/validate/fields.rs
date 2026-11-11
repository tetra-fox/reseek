//! Checks shared by many options: IP addresses, regular expressions, address ranges and URL
//! paths, besides the configuration library's own ranges, lengths and required values. Each
//! returns the message for a value that fails, naming the option by `name`.

use crate::config::addresses;
use crate::config::cidr::{IpRange, parse_list};

pub(crate) use cascade::checks::{
  at_least, duration_at_least, duration_off_or_at_least, http_url, length, off_or_at_least,
  optional_at_least, range, required,
};

/// One IP address, IPv4 or IPv6.
pub(crate) fn ip_address(name: &str, value: &str) -> Result<(), String> {
  if value.contains(',') {
    return Err(format!(
      "The field {name} accepts a single value, not a comma-separated list."
    ));
  }
  if addresses::address(value.trim()).is_none() {
    return Err(format!(
      "The field {name} specifies an invalid IPv4 or IPv6 address."
    ));
  }
  Ok(())
}

/// A comma-separated list of IP addresses as the listeners read it.
pub(crate) fn ip_addresses(name: &str, value: &str) -> Result<(), String> {
  addresses::list(Some(value))
    .map(drop)
    .map_err(|_| format!("The field {name} contains one or more invalid IPv4 or IPv6 addresses."))
}

/// Whether `pattern` is a usable regular expression: not blank, and it compiles as the engines
/// compile it, matching ignoring case unless `case_sensitive`.
pub(crate) fn is_regex(pattern: &str, case_sensitive: bool) -> bool {
  reseek_shares::patterns::compile(pattern, case_sensitive).is_ok()
}

/// One address range.
pub(crate) fn cidr(value: &str) -> Result<(), String> {
  value
    .parse::<IpRange>()
    .map(drop)
    .map_err(|e| format!("CIDR {value} is invalid: {e}."))
}

/// A comma-separated list of address ranges.
pub(crate) fn cidr_list(value: &str) -> Result<(), String> {
  parse_list(value)
    .map(drop)
    .map_err(|(element, e)| format!("CIDR {element} is invalid: {e}."))
}

/// A path the HTTP server serves under: no `?`, `#`, whitespace or `..`.
pub(crate) fn url_path(name: &str, value: &str) -> Result<(), String> {
  if value.contains(['?', '#']) || value.chars().any(char::is_whitespace) || value.contains("..") {
    return Err(format!(
      "The field {name} must not contain '?', '#', whitespace or '..'."
    ));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ip_addresses_are_single_or_lists() {
    assert!(ip_address("x", "0.0.0.0").is_ok());
    assert!(ip_address("x", "::1").is_ok());
    assert_eq!(
      ip_address("x", "1.2.3.4,5.6.7.8").unwrap_err(),
      "The field x accepts a single value, not a comma-separated list."
    );
    assert!(ip_address("x", "localhost").is_err());
    assert_eq!(
      ip_addresses("x", "127.0.0.1,nope").unwrap_err(),
      "The field x contains one or more invalid IPv4 or IPv6 addresses."
    );
  }

  #[test]
  fn url_paths_refuse_query_fragment_space_and_dots() {
    assert!(url_path("x", "/reseek/").is_ok());
    assert!(url_path("x", "reseek").is_ok());
    for bad in ["/a?b", "/a#b", "/a b", "/a/../b"] {
      assert!(url_path("x", bad).is_err(), "{bad}");
    }
  }

  #[test]
  fn regexes_and_cidrs_report_the_value() {
    assert!(is_regex("^.*\\.flac$", false));
    assert!(!is_regex("(", false));
    assert!(!is_regex("  ", false));
    assert!(!is_regex("(?=x)", false), "look-around is not supported");
    assert_eq!(
      cidr("192.168.1.0/33").unwrap_err(),
      "CIDR 192.168.1.0/33 is invalid: prefix length 33 is out of range."
    );
    assert_eq!(
      cidr_list("0.0.0.0/0,::ffff:1.2.3.4").unwrap_err(),
      "CIDR ::ffff:1.2.3.4 is invalid: IPv4 mapped IPv6 addresses are not allowed."
    );
  }
}
