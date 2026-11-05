//! Whether a request in a buddy's name comes from the buddy. A peer can give any name when it
//! connects, so a buddy's view of our shares goes only to a request from the address the server
//! last reported for the name, which the client keeps for up to a minute. A buddy behind another
//! address, such as one on our own network, sees what everyone sees, and users behind one public
//! address cannot be told apart.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use api_types::shares::ShareVisibility;
use soulseek_client::ClientError;
use tracing::{debug, warn};

use super::Directory;
use crate::sync::lock;

/// How often a request in one buddy's name from one other address is warned of.
const WARN_EVERY: Duration = Duration::from_secs(60);

/// When a request in each buddy's name from each other address was last warned of, so a buddy
/// asking from our own network does not hide an impersonator from the log.
#[derive(Default)]
pub struct Impersonations(Mutex<HashMap<(String, IpAddr), Instant>>);

impl Impersonations {
  /// Whether a request in `username`'s name from `from` is warned of now; the addresses not
  /// warned of within the last minute are forgotten.
  fn allow(&self, username: &str, from: IpAddr) -> bool {
    let now = Instant::now();
    let mut warned = lock(&self.0);
    warned.retain(|_, at| now.duration_since(*at) < WARN_EVERY);
    match warned.entry((username.to_string(), from)) {
      Entry::Occupied(_) => false,
      Entry::Vacant(entry) => {
        entry.insert(now);
        true
      }
    }
  }
}

/// The address that confirms a request in a buddy's name, given the address the server reports
/// for them and the one the request came from: the reported one, when the request came from it
/// or, with `from` unknown, its answer goes there; else the other address. An IPv4 address and
/// the same address mapped into IPv6 are one address.
fn confirmation(reported: IpAddr, from: Option<IpAddr>) -> Result<IpAddr, IpAddr> {
  let reported = reported.to_canonical();
  match from.map(|ip| ip.to_canonical()) {
    Some(from) if from != reported => Err(from),
    _ => Ok(reported),
  }
}

impl Directory {
  /// The narrowest shares a request in `username`'s name from `from` sees: a buddy's only when
  /// `from` is the address the server reports for them, else everyone's.
  pub async fn sees_from(&self, username: &str, from: IpAddr) -> ShareVisibility {
    self
      .confirmed(username, Some(from))
      .await
      .map_or(ShareVisibility::Everyone, |(sees, _)| sees)
  }

  /// The narrowest shares a search in `username`'s name sees, whose answer goes out on the
  /// message connection open from `open` or, with none open, on one to the address the server
  /// reports; with the only address the answer may go to when it holds more than everyone sees.
  pub async fn search_sees(
    &self,
    username: &str,
    open: Option<IpAddr>,
  ) -> (ShareVisibility, Option<IpAddr>) {
    match self.confirmed(username, open).await {
      Some((sees, at)) => (sees, Some(at)),
      None => (ShareVisibility::Everyone, None),
    }
  }

  /// What `username` sees as a buddy, with the address that confirms the name; `None` for a
  /// user who is no buddy, whose address the server does not report, or who asks from another.
  async fn confirmed(
    &self,
    username: &str,
    from: Option<IpAddr>,
  ) -> Option<(ShareVisibility, IpAddr)> {
    let standing = self.sees(username);
    if standing == ShareVisibility::Everyone {
      return None;
    }
    // the client is attached before the network answers anyone
    let client = self.client.get()?;
    let reported = match client.get_user_endpoint(username).await {
      Ok(endpoint) => endpoint.ip(),
      Err(ClientError::UserOffline(_)) => {
        debug!(
          username,
          "a buddy the server says is offline; answered as everyone"
        );
        return None;
      }
      Err(e) => {
        debug!(username, error = %e, "a buddy's address not found; answered as everyone");
        return None;
      }
    };
    match confirmation(reported, from) {
      Ok(at) => Some((standing, at)),
      Err(from) => {
        if self.impersonations.allow(username, from) {
          warn!(
            username,
            %reported,
            %from,
            "a request in a buddy's name came from another address than the server reports; \
             answered as everyone"
          );
        }
        None
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_buddys_name_is_confirmed_only_from_the_address_the_server_reports() {
    let ip = |text: &str| -> IpAddr { text.parse().unwrap() };
    let reported = ip("203.0.113.5");
    assert_eq!(confirmation(reported, Some(reported)), Ok(reported));
    assert_eq!(
      confirmation(reported, Some(ip("::ffff:203.0.113.5"))),
      Ok(reported),
      "a dual-stack listener's mapped address"
    );
    assert_eq!(
      confirmation(reported, None),
      Ok(reported),
      "the answer goes there"
    );
    assert_eq!(
      confirmation(reported, Some(ip("192.168.1.20"))),
      Err(ip("192.168.1.20")),
      "a buddy on our own network is not confirmed either"
    );
  }

  #[test]
  fn an_address_is_warned_of_once_a_minute_for_each_name() {
    let warned = Impersonations::default();
    let lan: IpAddr = "192.168.1.20".parse().unwrap();
    let elsewhere: IpAddr = "198.51.100.7".parse().unwrap();
    assert!(warned.allow("alice", lan));
    assert!(!warned.allow("alice", lan));
    assert!(
      warned.allow("alice", elsewhere),
      "another address is its own line"
    );
    assert!(warned.allow("bob", lan));
  }
}
