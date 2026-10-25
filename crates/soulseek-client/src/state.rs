//! What the client exposes about its server session: the connection state, the tuning values
//! the server sends, and the shape of the distributed network snapshot.

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};

bitflags::bitflags! {
  /// Where the client is in its server session. Only these combinations occur: `DISCONNECTED`,
  /// `CONNECTING`, `CONNECTED`, `CONNECTED | LOGGING_IN`, `CONNECTED | LOGGED_IN` and
  /// `DISCONNECTING`.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct ClientState: u8 {
    /// no server connection
    const DISCONNECTED = 1;
    /// the TCP connection to the server is up
    const CONNECTED = 2;
    /// the server accepted our login
    const LOGGED_IN = 4;
    /// the TCP connect to the server is in progress
    const CONNECTING = 8;
    /// the login was sent and its reply has not arrived
    const LOGGING_IN = 16;
    /// the session is being torn down
    const DISCONNECTING = 32;
  }
}

impl ClientState {
  /// connected and logged in, the state server requests need
  pub const READY: Self = Self::CONNECTED.union(Self::LOGGED_IN);
}

/// the flag names joined with ", ", as in "Connected, LoggedIn"
impl fmt::Display for ClientState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write_flags(
      f,
      *self,
      &[
        (Self::DISCONNECTED, "Disconnected"),
        (Self::CONNECTED, "Connected"),
        (Self::LOGGED_IN, "LoggedIn"),
        (Self::CONNECTING, "Connecting"),
        (Self::LOGGING_IN, "LoggingIn"),
        (Self::DISCONNECTING, "Disconnecting"),
      ],
    )
  }
}

/// Writes the names of the flags of `value` in the order of `names`, joined with ", ", or
/// "None" when none is set
pub(crate) fn write_flags<F: bitflags::Flags + Copy>(
  f: &mut fmt::Formatter<'_>,
  value: F,
  names: &[(F, &str)],
) -> fmt::Result {
  let mut first = true;
  for (flag, name) in names {
    if value.contains(*flag) {
      if !first {
        f.write_str(", ")?;
      }
      f.write_str(name)?;
      first = false;
    }
  }
  if first {
    f.write_str("None")?;
  }
  Ok(())
}

/// Reads flag names joined with ", " back, as [`write_flags`] writes them; `None` for a name not
/// in `names`
pub(crate) fn parse_flags<F: bitflags::Flags + Copy>(text: &str, names: &[(F, &str)]) -> Option<F> {
  if text == "None" {
    return Some(F::empty());
  }
  text.split(", ").try_fold(F::empty(), |flags, part| {
    let (flag, _) = names.iter().find(|(_, name)| *name == part)?;
    Some(flags.union(*flag))
  })
}

/// Tuning values the server sends after login, each `None` until its message arrives. The last
/// four come only from older servers; they are kept for diagnostics and never acted on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerInfo {
  /// the upload speed, in bytes/second, a client needs to be a distributed parent
  pub parent_min_speed: Option<u32>,
  /// upload speed divided by this is the number of children a parent may take
  pub parent_speed_ratio: Option<u32>,
  /// seconds between wishlist searches
  pub wishlist_interval: Option<u32>,
  /// the account has ever bought privileges
  pub is_supporter: Option<bool>,
  /// our address as the server sees it, from the login reply
  pub own_public_ip: Option<Ipv4Addr>,
  /// seconds
  pub parent_inactivity_timeout: Option<u32>,
  /// seconds
  pub search_inactivity_timeout: Option<u32>,
  /// how many parent candidates the server wanted a client to keep
  pub minimum_parents_in_cache: Option<u32>,
  /// seconds
  pub distributed_alive_interval: Option<u32>,
}

/// Our place in the distributed search tree, as the distributed network reports it
#[derive(Clone, Debug, PartialEq)]
pub struct DistributedNetworkInfo {
  /// milliseconds, a moving average of how long passing a search to every child takes
  pub average_broadcast_latency: Option<f64>,
  /// our depth in the tree: our parent's level plus one, or 0 without a parent
  pub branch_level: u32,
  /// the username at the top of our branch: our parent's root, or our own name without a parent
  pub branch_root: String,
  /// the server sends us searches directly, so we are the top of a branch
  pub is_branch_root: bool,
  /// the most children we take, the configured limit capped by our upload speed
  pub child_limit: u32,
  /// whether we tell the server we take children
  pub can_accept_children: bool,
  /// the username and address of every established child
  pub children: Vec<(String, SocketAddr)>,
  /// the parent's username and address; an empty name and no address when we have none
  pub parent: (String, Option<SocketAddr>),
  /// a parent is connected
  pub has_parent: bool,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn state_displays_as_flag_names() {
    assert_eq!(ClientState::READY.to_string(), "Connected, LoggedIn");
    assert_eq!(ClientState::DISCONNECTED.to_string(), "Disconnected");
  }
}
