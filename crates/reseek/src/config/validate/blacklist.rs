//! The blocklist file's format, told from its first line that is not blank or a comment.
//!
//! - CIDR: one address, network or range per line, such as `1.2.4.0/24`.
//! - P2P: `<description>:<first>-<last>`; the range is everything after the last `:`.
//! - DAT: `<first> - <last> , <level> , <description>` with zero-padded octets; the range is
//!   the text before the first `,`.

use crate::config::cidr::IpRange;

/// A blocklist file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
  Cidr,
  P2p,
  Dat,
}

/// The message for a file whose format can not be told.
pub(crate) const UNDETECTED: &str =
  "Failed to detect blacklist format. Only CIDR, P2P and DAT formats are supported";

/// The format of a blocklist with this text, from its first data line.
pub(crate) fn detect(text: &str) -> Option<Format> {
  let line = text
    .lines()
    .map(str::trim)
    .find(|line| !line.is_empty() && !line.starts_with('#'))?;
  if line.parse::<IpRange>().is_ok() {
    return Some(Format::Cidr);
  }
  if let Some((_, range)) = line.rsplit_once(':')
    && is_range(range)
  {
    return Some(Format::P2p);
  }
  let first = line.split(',').next().unwrap_or(line);
  let range: String = first.chars().filter(|c| !c.is_whitespace()).collect();
  is_range(&without_leading_zeros(&range)).then_some(Format::Dat)
}

/// A `first-last` range.
fn is_range(text: &str) -> bool {
  text.contains('-') && text.parse::<IpRange>().is_ok()
}

/// The range with each IPv4 octet's leading zeros dropped, `001.002.004.000` to `1.2.4.0`.
fn without_leading_zeros(range: &str) -> String {
  range
    .split('-')
    .map(|address| {
      address
        .split('.')
        .map(|octet| {
          let trimmed = octet.trim_start_matches('0');
          if trimmed.is_empty() && !octet.is_empty() {
            "0"
          } else {
            trimmed
          }
        })
        .collect::<Vec<_>>()
        .join(".")
    })
    .collect::<Vec<_>>()
    .join("-")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn formats_are_told_from_the_first_data_line() {
    assert_eq!(detect("# list\n\n1.2.4.0/24\n"), Some(Format::Cidr));
    assert_eq!(detect("1.2.4.7"), Some(Format::Cidr));
    assert_eq!(
      detect("China Internet Information Center (CNNIC):1.2.4.0-1.2.4.255"),
      Some(Format::P2p)
    );
    assert_eq!(
      detect("Foo: bar:1.2.3.4-1.2.3.5"),
      Some(Format::P2p),
      "the range follows the last colon"
    );
    assert_eq!(
      detect("001.002.004.000 - 001.002.004.255 , 000 , Botnet on Telekom Malaysia"),
      Some(Format::Dat)
    );
    assert_eq!(detect("# only comments\n\n"), None);
    assert_eq!(detect(""), None);
    assert_eq!(detect("hello world"), None);
  }
}
