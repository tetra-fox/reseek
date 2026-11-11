//! What the client exposes about its server session: the connection state, the tuning values
//! the server sends, and the shape of the distributed network snapshot.

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};

/// Where the client is in its server session. A session goes `Connecting`, `Connected`,
/// `LoggingIn`, `LoggedIn`, and from any of those to `Disconnecting` and `Disconnected`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClientState {
  /// no server connection
  Disconnected,
  /// the TCP connect to the server is in progress
  Connecting,
  /// the TCP connection to the server is up and no login was sent yet
  Connected,
  /// the login was sent and its reply has not arrived
  LoggingIn,
  /// the server accepted our login: server requests can be made
  LoggedIn,
  /// the session is being torn down
  Disconnecting,
}

impl ClientState {
  /// The state's name as the logs and the API write it: `"Connected, LoggingIn"` and
  /// `"Connected, LoggedIn"` for the two that hold a connection and a login.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Disconnected => "Disconnected",
      Self::Connecting => "Connecting",
      Self::Connected => "Connected",
      Self::LoggingIn => "Connected, LoggingIn",
      Self::LoggedIn => "Connected, LoggedIn",
      Self::Disconnecting => "Disconnecting",
    }
  }

  /// the server accepted our login
  pub fn is_logged_in(self) -> bool {
    self == Self::LoggedIn
  }

  /// the TCP connection to the server is up: connected, logging in or logged in
  pub fn is_connected(self) -> bool {
    matches!(self, Self::Connected | Self::LoggingIn | Self::LoggedIn)
  }

  /// on the way from one settled state to another: connecting, logging in or disconnecting
  pub fn is_transitioning(self) -> bool {
    matches!(
      self,
      Self::Connecting | Self::LoggingIn | Self::Disconnecting
    )
  }
}

impl fmt::Display for ClientState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.as_str())
  }
}

/// The server session read at one moment: which session it is, where it stands, and how the
/// latest session to end ended. A subscriber that missed events compares it with the last
/// session it handled to tell which edges it missed.
#[derive(Clone, Debug)]
pub struct SessionStatus {
  /// the session's number: sessions are counted from 1 as each connect begins, and 0 means
  /// none has begun
  pub number: u64,
  /// where the session stands
  pub state: ClientState,
  /// its TCP connection to the server came up
  pub reached: bool,
  /// our username, while logged in
  pub username: Option<String>,
  /// the end of the latest session that ended: this one once it has, else one before it
  pub last_end: Option<SessionEnd>,
}

/// How a server session ended, as its `Disconnected` event said
#[derive(Clone, Debug)]
pub struct SessionEnd {
  /// the session's number
  pub session: u64,
  /// what ended it
  pub message: String,
  /// the error that ended it, when one did
  pub error: Option<crate::error::ClientError>,
  /// when it ended
  pub at: std::time::SystemTime,
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
