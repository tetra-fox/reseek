//! The managed blocklist: IPv4 ranges read from a file in CIDR, P2P or DAT form, grouped by
//! their first octet so a lookup scans only the ranges that can hold the address.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::IpAddr;
use std::path::Path;

use crate::config::cidr::{IpRange, unmapped};
use crate::config::validate::blacklist::{Format, UNDETECTED, detect, without_leading_zeros};

/// How much of the file is read at a time.
const READ_BUFFER: usize = 256 * 1024;

/// The ranges of a blocklist file.
#[derive(Debug, Default)]
pub struct Blocklist {
  /// the ranges by the first octet they cover, each list sorted by its first address
  buckets: HashMap<u8, Vec<(u32, u32)>>,
  /// how many ranges the buckets hold, a range in several buckets counted in each
  count: usize,
}

impl Blocklist {
  /// Reads the blocklist at `path`, its format told from its first data line.
  pub fn load(path: &Path) -> Result<Self, String> {
    let head = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let format = detect(&String::from_utf8_lossy(&head)).ok_or_else(|| UNDETECTED.to_string())?;
    drop(head);
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buckets: HashMap<u8, Vec<(u32, u32)>> = HashMap::new();
    for (index, line) in BufReader::with_capacity(READ_BUFFER, file)
      .split(b'\n')
      .enumerate()
    {
      let line = line.map_err(|e| format!("{}: {e}", path.display()))?;
      let line = String::from_utf8_lossy(&line);
      let line = line.trim();
      if line.is_empty() || line.starts_with('#') {
        continue;
      }
      let (first, last) = range(format, line).map_err(|reason| {
        let name = match format {
          Format::Cidr => "CIDR",
          Format::P2p => "P2P",
          Format::Dat => "DAT",
        };
        format!(
          "Failed to parse {name} blacklist line {} '{line}': {reason}",
          index + 1
        )
      })?;
      for octet in (first >> 24)..=(last >> 24) {
        let octet = u8::try_from(octet).expect("the top byte of a u32");
        buckets.entry(octet).or_default().push((first, last));
      }
    }
    let mut count = 0;
    for ranges in buckets.values_mut() {
      ranges.sort_unstable();
      ranges.dedup();
      count += ranges.len();
    }
    Ok(Self { buckets, count })
  }

  /// How many ranges it holds.
  pub fn count(&self) -> usize {
    self.count
  }

  /// Whether `ip` is in one of the ranges. An IPv4-mapped address is looked up as IPv4; any
  /// other IPv6 address is in none.
  pub fn contains(&self, ip: IpAddr) -> bool {
    let IpAddr::V4(v4) = unmapped(ip) else {
      return false;
    };
    let address = u32::from(v4);
    self.buckets.get(&v4.octets()[0]).is_some_and(|ranges| {
      ranges
        .iter()
        .any(|&(first, last)| first <= address && address <= last)
    })
  }
}

/// The IPv4 range a line of `format` holds.
fn range(format: Format, line: &str) -> Result<(u32, u32), String> {
  let text = match format {
    Format::Cidr => line.to_string(),
    Format::P2p => line
      .rsplit_once(':')
      .map(|(_, range)| range.to_string())
      .ok_or("no ':' before the range")?,
    Format::Dat => {
      let first = line.split(',').next().unwrap_or(line);
      without_leading_zeros(
        &first
          .chars()
          .filter(|c| !c.is_whitespace())
          .collect::<String>(),
      )
    }
  };
  let parsed: IpRange = text.trim().parse().map_err(|e| format!("{e}"))?;
  match parsed.bounds() {
    (IpAddr::V4(first), IpAddr::V4(last)) => Ok((u32::from(first), u32::from(last))),
    _ => Err("not an IPv4 range".to_string()),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn load(text: &str) -> Result<Blocklist, String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blocklist");
    std::fs::write(&path, text).unwrap();
    Blocklist::load(&path)
  }

  #[test]
  fn each_format_is_read_and_looked_up_by_its_first_octet() {
    let cidr = load("# list\n1.2.4.0/24\n\n10.0.0.0/7\n").unwrap();
    assert!(cidr.contains("1.2.4.9".parse().unwrap()));
    assert!(!cidr.contains("1.2.5.9".parse().unwrap()));
    assert!(
      cidr.contains("11.1.1.1".parse().unwrap()),
      "a wide range spans buckets"
    );
    assert_eq!(cidr.count(), 3);
    assert!(cidr.contains("::ffff:1.2.4.1".parse().unwrap()));
    assert!(!cidr.contains("2001:db8::1".parse().unwrap()));

    let p2p = load("Some: Name (CNNIC):1.2.4.0-1.2.4.255\n").unwrap();
    assert!(p2p.contains("1.2.4.200".parse().unwrap()));
    let dat = load("001.002.004.000 - 001.002.004.255 , 000 , Botnet\n").unwrap();
    assert!(dat.contains("1.2.4.0".parse().unwrap()));
    let broken = load("1.2.4.0/24\nnot a range\n").unwrap_err();
    assert!(broken.contains("line 2 'not a range'"), "{broken}");
  }
}
