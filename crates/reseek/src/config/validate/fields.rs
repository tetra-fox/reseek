//! Checks shared by many options: ranges, string lengths, required values, IP addresses,
//! regular expressions, address ranges and URLs. Each returns the message for a value that
//! fails, naming the option by `name`.

use std::fmt::Display;
use std::time::Duration;

use crate::config::addresses;
use crate::config::cidr::{IpRange, parse_list};

/// `value` is from `min` to `max`, inclusive.
pub(crate) fn range<T: PartialOrd + Display>(
  name: &str,
  value: T,
  min: T,
  max: T,
) -> Result<(), String> {
  if value < min || value > max {
    return Err(format!("The field {name} must be between {min} and {max}."));
  }
  Ok(())
}

/// `value` is at least `min`; there is no upper limit.
pub(crate) fn at_least<T: PartialOrd + Display>(
  name: &str,
  value: T,
  min: T,
) -> Result<(), String> {
  if value < min {
    return Err(format!("The field {name} must be at least {min}."));
  }
  Ok(())
}

/// `value`, when set, is at least `min`.
pub(crate) fn optional_at_least<T: PartialOrd + Display + Copy>(
  name: &str,
  value: Option<T>,
  min: T,
) -> Result<(), String> {
  value.map_or(Ok(()), |v| at_least(name, v, min))
}

/// `value` is at least `min`.
pub(crate) fn duration_at_least(name: &str, value: Duration, min: Duration) -> Result<(), String> {
  if value < min {
    return Err(format!(
      "The field {name} must be at least {}.",
      humantime::format_duration(min)
    ));
  }
  Ok(())
}

/// `value` is `0`, which turns its limit off, or at least `min`.
pub(crate) fn duration_off_or_at_least(
  name: &str,
  value: Duration,
  min: Duration,
) -> Result<(), String> {
  if !value.is_zero() && value < min {
    return Err(format!(
      "The field {name} must be 0 to turn it off, or at least {}.",
      humantime::format_duration(min)
    ));
  }
  Ok(())
}

/// `value` is `0`, which turns its limit off, or at least `min`.
pub(crate) fn off_or_at_least(name: &str, value: u32, min: u32) -> Result<(), String> {
  if value != 0 && value < min {
    return Err(format!(
      "The field {name} must be 0 to turn it off, or at least {min}."
    ));
  }
  Ok(())
}

/// A string, when set, is from `min` to `max` characters long.
pub(crate) fn length(
  name: &str,
  value: Option<&str>,
  min: usize,
  max: usize,
) -> Result<(), String> {
  let Some(value) = value else {
    return Ok(());
  };
  let length = value.chars().count();
  if length < min || length > max {
    return Err(format!(
      "The field {name} must be a string with a minimum length of {min} and a maximum length of \
       {max}."
    ));
  }
  Ok(())
}

/// A string is set and holds more than whitespace.
pub(crate) fn required(name: &str, value: Option<&str>) -> Result<(), String> {
  match value {
    None | Some("") => Err(format!("The {name} field must contain a value")),
    Some(v) if v.trim().is_empty() => Err(format!(
      "The {name} field must not contain a value consisting only of whitespace"
    )),
    Some(_) => Ok(()),
  }
}

/// One IP address, IPv4 or IPv6.
pub(crate) fn ip_address(name: &str, value: &str) -> Result<(), String> {
  if value.contains(',') {
    return Err(format!(
      "The {name} field accepts a single value only (a comma separated list was specified)"
    ));
  }
  if addresses::address(value.trim()).is_none() {
    return Err(format!(
      "The {name} field specifies an invalid IPv4 or IPv6 IP address."
    ));
  }
  Ok(())
}

/// A comma-separated list of IP addresses as the listeners read it.
pub(crate) fn ip_addresses(name: &str, value: &str) -> Result<(), String> {
  addresses::list(Some(value)).map(drop).map_err(|_| {
    format!("The {name} field contains one or more invalid IPv4 or IPv6 IP addresses.")
  })
}

/// Whether `pattern` is a usable regular expression: not blank, and it compiles.
pub(crate) fn is_regex(pattern: &str) -> bool {
  !pattern.trim().is_empty() && regex::Regex::new(pattern).is_ok()
}

/// One address range.
pub(crate) fn cidr(value: &str) -> Result<(), String> {
  value
    .parse::<IpRange>()
    .map(drop)
    .map_err(|e| format!("CIDR {value} is invalid: {e}"))
}

/// A comma-separated list of address ranges.
pub(crate) fn cidr_list(value: &str) -> Result<(), String> {
  parse_list(value)
    .map(drop)
    .map_err(|(element, e)| format!("CIDR {element} is invalid: {e}"))
}

/// An absolute `http` or `https` URL with a host.
pub(crate) fn http_url(name: &str, value: &str) -> Result<(), String> {
  match url::Url::parse(value) {
    Ok(url) if matches!(url.scheme(), "http" | "https") && url.has_host() => Ok(()),
    _ => Err(format!(
      "The field {name} must be an absolute http or https URL."
    )),
  }
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
      "The x field accepts a single value only (a comma separated list was specified)"
    );
    assert!(ip_address("x", "localhost").is_err());
    assert!(ip_addresses("x", "127.0.0.1, [::1] ,::").is_ok());
    assert!(
      ip_addresses("x", "  ").is_err(),
      "only an absent list is every address"
    );
    assert!(ip_addresses("x", "").is_err());
    assert_eq!(
      ip_addresses("x", "127.0.0.1,nope").unwrap_err(),
      "The x field contains one or more invalid IPv4 or IPv6 IP addresses."
    );
  }

  #[test]
  fn urls_are_absolute_http() {
    assert!(http_url("x", "https://user:pw@host:5031/base").is_ok());
    assert!(http_url("x", "http://localhost:3100").is_ok());
    for bad in ["ftp://host", "host:5031", "/relative", "https://", ""] {
      assert!(http_url("x", bad).is_err(), "{bad}");
    }
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
    assert!(is_regex("^.*\\.flac$"));
    assert!(!is_regex("("));
    assert!(!is_regex("  "));
    assert!(!is_regex("(?=x)"), "look-around is not supported");
    assert_eq!(
      cidr("192.168.1.0/33").unwrap_err(),
      "CIDR 192.168.1.0/33 is invalid: prefix length 33 is out of range"
    );
    assert_eq!(
      cidr_list("0.0.0.0/0,::ffff:1.2.3.4").unwrap_err(),
      "CIDR ::ffff:1.2.3.4 is invalid: IPv4 mapped IPv6 addresses are not allowed"
    );
  }

  #[test]
  fn required_strings_need_more_than_whitespace() {
    assert_eq!(
      required("x", None).unwrap_err(),
      "The x field must contain a value"
    );
    assert_eq!(
      required("x", Some(" ")).unwrap_err(),
      "The x field must not contain a value consisting only of whitespace"
    );
    assert!(required("x", Some("a")).is_ok());
  }
}
