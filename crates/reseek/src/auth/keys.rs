//! API keys from the configuration: the single key given by environment variable or command
//! line, and the named keys of the files.

use std::collections::BTreeMap;
use std::net::IpAddr;

use api_types::auth::{Principal, Via};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::config::ApiKey;
use crate::config::api_key::PrimaryApiKey;
use crate::config::cidr::{IpRange, parse_list};

/// The name the single key from the environment or the command line goes by.
pub const PRIMARY: &str = "primary";

/// Why a presented key was refused.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeyRefusal {
  /// no configured key is the presented one
  #[error("Unknown API key")]
  Unknown,
  /// the key may not be used from the client's address
  #[error("API key {name} is not allowed from {ip}")]
  Address {
    /// the key's name
    name: String,
    /// the client's address
    ip: IpAddr,
  },
}

/// The principal the presented key `presented` stands for, used from `ip`. Every configured
/// key is compared, each in constant time, so the time taken does not tell which one matched.
pub fn check(
  presented: &str,
  ip: IpAddr,
  primary: Option<&PrimaryApiKey>,
  named: &BTreeMap<String, ApiKey>,
) -> Result<Principal, KeyRefusal> {
  let wanted = Sha256::digest(presented.as_bytes());
  let mut found: Option<(String, crate::config::enums::Role, Vec<IpRange>)> = None;
  let mut consider = |name: &str, key: &str, role, cidrs: &dyn Fn() -> Vec<IpRange>| {
    let matches: bool = Sha256::digest(key.as_bytes()).ct_eq(&wanted).into();
    if matches && found.is_none() {
      found = Some((name.to_string(), role, cidrs()));
    }
  };
  if let Some(primary) = primary {
    consider(PRIMARY, &primary.key, primary.role, &|| {
      primary.cidrs.clone()
    });
  }
  for (name, key) in named {
    let Some(secret) = &key.key else {
      continue;
    };
    consider(name, secret, key.role, &|| cidrs_of(name, &key.cidr));
  }
  let (name, role, cidrs) = found.ok_or(KeyRefusal::Unknown)?;
  if !cidrs.iter().any(|range| range.contains(ip)) {
    return Err(KeyRefusal::Address { name, ip });
  }
  Ok(Principal {
    subject: format!("apikey:{name}"),
    name: name.clone(),
    role: role.into(),
    via: Via::ConfigApiKey { name },
  })
}

/// The networks of a named key. They were checked when the configuration loaded, so a list
/// that does not parse allows nothing.
fn cidrs_of(name: &str, cidr: &str) -> Vec<IpRange> {
  match parse_list(cidr) {
    Ok(ranges) => ranges,
    Err((element, e)) => {
      tracing::error!(key = name, element, error = %e, "API key network does not parse");
      Vec::new()
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::enums::Role;

  fn named() -> BTreeMap<String, ApiKey> {
    BTreeMap::from([
      (
        "scripts".to_string(),
        ApiKey {
          key: Some("scripts-key-0123456789".into()),
          role: Role::ReadWrite,
          cidr: "192.168.1.0/24".into(),
        },
      ),
      ("empty".to_string(), ApiKey::default()),
    ])
  }

  #[test]
  fn keys_match_by_value_and_are_held_to_their_networks() {
    let lan: IpAddr = "192.168.1.20".parse().unwrap();
    let principal = check("scripts-key-0123456789", lan, None, &named()).unwrap();
    assert_eq!(principal.subject, "apikey:scripts");
    assert_eq!(principal.role, api_types::auth::Role::ReadWrite);
    assert_eq!(
      check(
        "scripts-key-0123456789",
        "10.0.0.1".parse().unwrap(),
        None,
        &named()
      ),
      Err(KeyRefusal::Address {
        name: "scripts".into(),
        ip: "10.0.0.1".parse().unwrap()
      })
    );
    assert_eq!(check("nope", lan, None, &named()), Err(KeyRefusal::Unknown));
    let primary = PrimaryApiKey::parse("role=readonly;primary-key-0123456789").unwrap();
    let principal = check("primary-key-0123456789", lan, Some(&primary), &named()).unwrap();
    assert_eq!(principal.subject, "apikey:primary");
    assert_eq!(principal.role, api_types::auth::Role::ReadOnly);
  }
}
