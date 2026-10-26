//! Address ranges in the forms the options accept: one address (`192.168.1.10`), a network
//! (`192.168.1.0/24`, `fd00::/8`), a network with a dotted mask (`10.0.0.0/255.0.0.0`) or an
//! inclusive range (`10.0.0.1-10.0.0.9`). IPv4-mapped IPv6 forms are refused; an IPv4-mapped
//! client address is matched as the IPv4 address it carries.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

/// Every IPv4 and IPv6 address, as a comma-separated list.
pub const ANY: &str = "0.0.0.0/0,::/0";

/// An inclusive range of addresses of one family, with the text it was read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpRange {
  start: IpAddr,
  end: IpAddr,
  text: String,
}

/// Why a range did not parse; the message completes `CIDR <x> is invalid: `.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RangeError {
  /// the text is no address, network or range
  #[error("not an address, a network or a range")]
  Syntax,
  /// the prefix length is longer than the address
  #[error("prefix length {0} is out of range")]
  Prefix(u32),
  /// a dotted mask with a gap in its bits
  #[error("the mask is not contiguous")]
  Mask,
  /// the two ends of a range are of different families or in the wrong order
  #[error("the range's ends are of different families or out of order")]
  Order,
  /// an IPv4-mapped IPv6 form
  #[error("IPv4 mapped IPv6 addresses are not allowed")]
  Mapped,
}

impl IpRange {
  /// The first and the last address of the range.
  pub fn bounds(&self) -> (IpAddr, IpAddr) {
    (self.start, self.end)
  }

  /// Whether `ip` is in the range. An IPv4-mapped IPv6 address is matched as its IPv4 address.
  pub fn contains(&self, ip: IpAddr) -> bool {
    match (unmapped(ip), self.start, self.end) {
      (IpAddr::V4(ip), IpAddr::V4(start), IpAddr::V4(end)) => start <= ip && ip <= end,
      (IpAddr::V6(ip), IpAddr::V6(start), IpAddr::V6(end)) => start <= ip && ip <= end,
      _ => false,
    }
  }
}

impl fmt::Display for IpRange {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.text)
  }
}

impl FromStr for IpRange {
  type Err = RangeError;

  fn from_str(text: &str) -> Result<Self, RangeError> {
    let (start, end) = if let Some((address, suffix)) = text.split_once('/') {
      let address = address_of(address)?;
      let prefix = match suffix.parse::<u32>() {
        Ok(prefix) => prefix,
        Err(_) => mask_prefix(address, suffix)?,
      };
      network(address, prefix)?
    } else if let Some((first, last)) = text.split_once('-') {
      let (first, last) = (address_of(first)?, address_of(last)?);
      if first.is_ipv4() != last.is_ipv4() || first > last {
        return Err(RangeError::Order);
      }
      (first, last)
    } else {
      let address = address_of(text)?;
      (address, address)
    };
    Ok(Self {
      start,
      end,
      text: text.to_string(),
    })
  }
}

/// Reads a comma-separated list of ranges. Elements are taken as written, not trimmed.
pub fn parse_list(text: &str) -> Result<Vec<IpRange>, (String, RangeError)> {
  text
    .split(',')
    .map(|element| element.parse().map_err(|e| (element.to_string(), e)))
    .collect()
}

/// The address an IPv4-mapped IPv6 address carries, else the address itself.
pub fn unmapped(ip: IpAddr) -> IpAddr {
  match ip {
    IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
    v4 => v4,
  }
}

fn address_of(text: &str) -> Result<IpAddr, RangeError> {
  let address: IpAddr = text.parse().map_err(|_| RangeError::Syntax)?;
  match address {
    IpAddr::V6(v6) if v6.to_ipv4_mapped().is_some() => Err(RangeError::Mapped),
    address => Ok(address),
  }
}

/// The prefix length a dotted IPv4 mask such as `255.255.0.0` stands for.
fn mask_prefix(address: IpAddr, mask: &str) -> Result<u32, RangeError> {
  let (IpAddr::V4(_), Ok(mask)) = (address, mask.parse::<Ipv4Addr>()) else {
    return Err(RangeError::Syntax);
  };
  let bits = u32::from(mask);
  let prefix = bits.leading_ones();
  if bits.checked_shl(prefix).unwrap_or(0) != 0 {
    return Err(RangeError::Mask);
  }
  Ok(prefix)
}

/// The first and last address of the network of `address` with `prefix` leading bits.
fn network(address: IpAddr, prefix: u32) -> Result<(IpAddr, IpAddr), RangeError> {
  match address {
    IpAddr::V4(v4) => {
      if prefix > 32 {
        return Err(RangeError::Prefix(prefix));
      }
      let host = u32::MAX.checked_shr(prefix).unwrap_or(0);
      let start = u32::from(v4) & !host;
      Ok((
        IpAddr::V4(Ipv4Addr::from(start)),
        IpAddr::V4(Ipv4Addr::from(start | host)),
      ))
    }
    IpAddr::V6(v6) => {
      if prefix > 128 {
        return Err(RangeError::Prefix(prefix));
      }
      let host = u128::MAX.checked_shr(prefix).unwrap_or(0);
      let start = u128::from(v6) & !host;
      Ok((
        IpAddr::V6(Ipv6Addr::from(start)),
        IpAddr::V6(Ipv6Addr::from(start | host)),
      ))
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
  }

  #[test]
  fn ranges_parse_in_every_form() {
    for valid in [
      "0.0.0.0/0",
      "1.2.3.4/32",
      "::/0",
      "fd00::/8",
      "10.0.0.0/255.0.0.0",
      "10.0.0.1-10.0.0.9",
      "192.168.1.10",
    ] {
      assert!(valid.parse::<IpRange>().is_ok(), "{valid}");
    }
    assert_eq!(
      "999.999.999.999/24".parse::<IpRange>(),
      Err(RangeError::Syntax)
    );
    assert_eq!(
      "192.168.1.0/33".parse::<IpRange>(),
      Err(RangeError::Prefix(33))
    );
    assert_eq!("not-a-cidr".parse::<IpRange>(), Err(RangeError::Syntax));
    assert_eq!(
      "::ffff:10.0.0.0/104".parse::<IpRange>(),
      Err(RangeError::Mapped)
    );
    assert_eq!(
      "10.0.0.0/255.0.255.0".parse::<IpRange>(),
      Err(RangeError::Mask)
    );
    assert_eq!(
      "10.0.0.9-10.0.0.1".parse::<IpRange>(),
      Err(RangeError::Order)
    );
  }

  #[test]
  fn ranges_contain_their_addresses_and_unmap_ipv4() {
    let lan: IpRange = "192.168.1.0/24".parse().unwrap();
    assert!(lan.contains(ip("192.168.1.255")));
    assert!(!lan.contains(ip("192.168.2.0")));
    assert!(lan.contains(ip("::ffff:192.168.1.7")));
    assert!(!lan.contains(ip("fe80::1")));
    let all = parse_list(ANY).unwrap();
    assert!(all.iter().any(|r| r.contains(ip("8.8.8.8"))));
    assert!(all.iter().any(|r| r.contains(ip("2001:db8::1"))));
    let range: IpRange = "10.0.0.1-10.0.0.9".parse().unwrap();
    assert!(range.contains(ip("10.0.0.5")) && !range.contains(ip("10.0.0.10")));
    assert_eq!(
      parse_list("10.0.0.0/8, 11.0.0.0/8").unwrap_err().0,
      " 11.0.0.0/8",
      "elements are not trimmed"
    );
  }
}
