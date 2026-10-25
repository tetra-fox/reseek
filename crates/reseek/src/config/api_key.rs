//! The single API key given by environment variable or command line. Its value is the key
//! alone, or `;`-separated parts: `role=<role>`, `cidr=<networks>` and the key itself, such as
//! `role=readonly;cidr=192.168.1.0/24;<key>`. Without them the key is an administrator's,
//! usable from anywhere.

use std::fmt;

use super::cidr::{ANY, IpRange, parse_list};
use super::enums::Role;

/// The single API key given by environment variable or command line, parsed.
#[derive(Clone, PartialEq, Eq)]
pub struct PrimaryApiKey {
  /// the key a request presents
  pub key: String,
  /// what the key may do
  pub role: Role,
  /// the networks it may be used from
  pub cidrs: Vec<IpRange>,
}

/// Leaves the key out, so it is never logged.
impl fmt::Debug for PrimaryApiKey {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("PrimaryApiKey")
      .field("role", &self.role)
      .field("cidrs", &self.cidrs)
      .finish_non_exhaustive()
  }
}

const LENGTH: &str = "API key must be between 16 and 255 characters";

impl PrimaryApiKey {
  /// Reads the key and its parts. The message of an error never quotes the key.
  pub fn parse(text: &str) -> Result<Self, String> {
    let mut key = None;
    let mut role = None;
    let mut cidrs = None;
    for part in text.split(';') {
      if let Some(name) = strip_prefix_ignore_case(part, "role=") {
        if role.is_some() {
          return Err("API key may have only one role= part".to_string());
        }
        let parsed = Role::parse(name).ok_or_else(|| {
          "API key role must be one of: ReadOnly, ReadWrite, Administrator".to_string()
        })?;
        role = Some(parsed);
      } else if let Some(list) = strip_prefix_ignore_case(part, "cidr=") {
        if cidrs.is_some() {
          return Err("API key may have only one cidr= part".to_string());
        }
        let parsed = parse_list(list)
          .map_err(|(element, e)| format!("API key CIDR {element} is invalid: {e}"))?;
        cidrs = Some(parsed);
      } else {
        if part.contains('=') {
          return Err(
            "API key parts are role=, cidr= and the key, which must not contain '=' or ';'"
              .to_string(),
          );
        }
        if key.is_some() {
          return Err("API key must have exactly one part that is the key".to_string());
        }
        key = Some(part);
      }
    }
    let key =
      key.ok_or_else(|| "API key must have exactly one part that is the key".to_string())?;
    if !(16..=255).contains(&key.chars().count()) {
      return Err(LENGTH.to_string());
    }
    let cidrs = match cidrs {
      Some(cidrs) => cidrs,
      None => parse_list(ANY).expect("every address parses"),
    };
    Ok(Self {
      key: key.to_string(),
      role: role.unwrap_or(Role::Administrator),
      cidrs,
    })
  }
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
  let head = text.get(..prefix.len())?;
  head
    .eq_ignore_ascii_case(prefix)
    .then(|| &text[prefix.len()..])
}

#[cfg(test)]
mod tests {
  use super::*;

  const KEY: &str = "0123456789abcdefXYZ";

  #[test]
  fn the_key_alone_is_an_administrators_from_anywhere() {
    let parsed = PrimaryApiKey::parse(KEY).unwrap();
    assert_eq!(parsed.key, KEY);
    assert_eq!(parsed.role, Role::Administrator);
    assert_eq!(parsed.cidrs.len(), 2);
    assert_eq!(PrimaryApiKey::parse("short").unwrap_err(), LENGTH);
  }

  #[test]
  fn parts_set_the_role_and_networks_in_any_order() {
    let parsed = PrimaryApiKey::parse(&format!("ROLE=readonly;{KEY};cidr=192.168.1.0/24")).unwrap();
    assert_eq!(parsed.key, KEY);
    assert_eq!(parsed.role, Role::ReadOnly);
    assert!(parsed.cidrs[0].contains("192.168.1.9".parse().unwrap()));
    assert!(!parsed.cidrs[0].contains("10.0.0.1".parse().unwrap()));
  }

  #[test]
  fn the_key_part_is_required_once_and_plain() {
    let no_key = PrimaryApiKey::parse("role=ReadOnly;cidr=10.0.0.0/8").unwrap_err();
    assert_eq!(no_key, "API key must have exactly one part that is the key");
    assert_eq!(
      PrimaryApiKey::parse(&format!("{KEY};{KEY}")).unwrap_err(),
      no_key
    );
    assert_eq!(
      PrimaryApiKey::parse(&format!("role=readonly;role=administrator;{KEY}")).unwrap_err(),
      "API key may have only one role= part"
    );
    assert_eq!(
      PrimaryApiKey::parse(&format!("cidr=10.0.0.0/8;cidr=::/0;{KEY}")).unwrap_err(),
      "API key may have only one cidr= part"
    );
    assert!(
      PrimaryApiKey::parse("0123456789abcdef=ghij")
        .unwrap_err()
        .contains("must not contain '='")
    );
    assert_eq!(
      PrimaryApiKey::parse(&format!("role=guest;{KEY}")).unwrap_err(),
      "API key role must be one of: ReadOnly, ReadWrite, Administrator"
    );
    assert_eq!(
      PrimaryApiKey::parse(&format!("cidr=10.0.0.0/33;{KEY}")).unwrap_err(),
      "API key CIDR 10.0.0.0/33 is invalid: prefix length 33 is out of range"
    );
    let unparsed = format!("{:?}", PrimaryApiKey::parse(KEY).unwrap());
    assert!(!unparsed.contains(KEY), "the key is not in the debug form");
  }
}
