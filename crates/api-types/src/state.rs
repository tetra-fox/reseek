//! The application state document: everything the UI shows about the daemon itself, sent whole
//! on every change.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::flags::flag_names;
use crate::relay::RelayState;

/// The daemon's state, as `GET /application` returns it and `application.state` frames carry
/// it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct State {
  /// the running version and whether a newer one exists
  pub version: VersionInfo,
  /// an option changed that takes effect only after reconnecting to the Soulseek server
  pub pending_reconnect: bool,
  /// an option changed that takes effect only after a restart
  pub pending_restart: bool,
  /// the connection to the Soulseek server
  pub server: ServerState,
  /// the loop that reconnects to the server
  pub connection_watchdog: ConnectionWatchdogState,
  /// relay mode, the controller connection and the connected agents
  pub relay: RelayState,
  /// sampled health figures
  pub health: HealthState,
  /// our own Soulseek account
  pub user: UserState,
  /// our place in the distributed search network
  pub distributed_network: DistributedNetworkState,
  /// the share scan and the share cache
  pub shares: SharesState,
  /// the chat rooms we are in
  pub rooms: Vec<String>,
  /// the jobs running and paused, and the last failure
  pub jobs: crate::jobs::JobsSummary,
  /// the alternative speed limits are in force in place of the normal ones
  pub alternative_speed_limits: bool,
  /// the outcome of the last configuration reload
  pub configuration: ConfigurationState,
  /// where finished and unfinished downloads are kept on this machine
  pub directories: DirectoriesState,
  /// when this process started
  pub started_at: DateTime<Utc>,
  /// this process and the system it runs on
  pub process: ProcessState,
  /// where the web UI and the API take requests: a URL per HTTP and HTTPS address, and
  /// `unix:` with the path of the unix domain socket
  #[schema(example = json!(["http://0.0.0.0:5030/"]))]
  pub listeners: Vec<String>,
}

/// This process and the system it runs on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessState {
  /// the process id
  pub pid: u32,
  /// the operating system
  #[schema(example = "linux")]
  pub os: String,
  /// the processor architecture
  #[schema(example = "x86_64")]
  pub arch: String,
  /// the memory the process holds in RAM, in bytes, sampled every 30 seconds
  pub memory_bytes: u64,
  /// how many files are analyzed at once when the analysis workers option is `auto`: half of
  /// the processors, at least one
  #[schema(example = 4)]
  pub auto_analysis_workers: u32,
  /// the largest file, in bytes, taken from a browser upload for analysis, as the options in
  /// effect set it
  #[schema(example = 2_000_000_000)]
  pub upload_max_bytes: u64,
}

/// The running version and whether a newer one exists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
  /// the version with its build information
  #[schema(example = "0.1.0 (0.1.0+a1b2c3d)")]
  pub full: String,
  /// the semantic version
  #[schema(example = "0.1.0")]
  pub current: String,
  /// the latest released version, null until the release feed was read
  pub latest: Option<String>,
  /// whether `latest` is newer than `current`, null while unknown
  pub is_update_available: Option<bool>,
  /// a canary build, which is not checked for updates
  pub is_canary: bool,
  /// a development build, which is not checked for updates
  pub is_development: bool,
}

bitflags::bitflags! {
  /// The state of the connection to the Soulseek server, written as the names of the set
  /// flags such as `"Connected, LoggedIn"`, or `"None"` before the first change.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct ServerConnectionState: u32 {
    /// not connected
    const Disconnected = 1;
    /// the TCP connection is up
    const Connected = 2;
    /// the server accepted our login
    const LoggedIn = 4;
    /// the TCP connection is being made
    const Connecting = 8;
    /// the login was sent and is not answered yet
    const LoggingIn = 16;
    /// the connection is being closed
    const Disconnecting = 32;
  }
}

flag_names!(
  ServerConnectionState,
  "the names of the set flags joined by \", \"",
  "Connected, LoggedIn"
);

/// The connection to the Soulseek server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerState {
  /// the configured server host while not disconnected
  pub address: Option<String>,
  /// the server's resolved `ip:port` while connected
  #[schema(example = "1.2.3.4:2271")]
  pub ip_end_point: Option<String>,
  /// the client's state flags
  pub state: ServerConnectionState,
  /// `state` has `Connected`
  pub is_connected: bool,
  /// `state` has `Connecting`
  pub is_connecting: bool,
  /// `state` has `LoggedIn`
  pub is_logged_in: bool,
  /// `state` has `LoggingIn`
  pub is_logging_in: bool,
  /// `state` has `Connecting`, `Disconnecting` or `LoggingIn`
  pub is_transitioning: bool,
  /// how other users reach our peer listener
  pub listen: ListenState,
}

/// How other users reach our peer listener: its port, the router forwarding it and whether
/// peers can connect to us.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListenState {
  /// the port the listener takes connections on, which the server tells other users
  pub port: u16,
  /// the router forwarding that port to us
  pub port_mapping: PortMappingState,
  /// whether other users can connect to us, judged from who has connected so far
  pub reachable: Reachable,
}

/// The router forwarding our listening port to us.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PortMappingState {
  /// where asking the router stands
  pub state: PortMappingStatus,
  /// how the router was asked, null until a mapping holds
  pub method: Option<PortMappingMethod>,
  /// our address on the internet as the router reports it, null when unknown
  #[schema(example = "203.0.113.7")]
  pub external_address: Option<String>,
  /// the port the router forwards to us, null until a mapping holds
  pub external_port: Option<u16>,
  /// why the router did not forward the port, null unless `state` is `failed`
  pub error: Option<String>,
}

/// Where asking the router to forward our port stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PortMappingStatus {
  /// port mapping is turned off, or we are not connected to the server
  Off,
  /// the router is being asked
  Mapping,
  /// the router forwards the port
  Mapped,
  /// no router answered, or it refused
  Failed,
}

/// How the router was asked to forward our port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PortMappingMethod {
  /// UPnP Internet Gateway Device
  Upnp,
  /// NAT Port Mapping Protocol
  Natpmp,
  /// Port Control Protocol
  Pcp,
}

/// Whether other users can connect to us.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Reachable {
  /// a peer has connected to our listener by itself
  Yes,
  /// too little has happened to tell
  Unknown,
  /// for ten minutes, or over five searches that drew responses, every connection to a peer
  /// was one we opened ourselves
  No,
}

/// The loop that reconnects to the Soulseek server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionWatchdogState {
  /// the loop is active and keeps trying
  pub is_enabled: bool,
  /// a connection attempt is waiting or running
  pub is_attempting_connection: bool,
  /// when the next attempt starts, null when none is scheduled
  pub next_attempt_at: Option<DateTime<Utc>>,
  /// the server did not answer the last login, which is how it treats a banned account; the
  /// attempts are then 15 minutes, an hour and after that 6 hours apart
  pub is_login_unanswered: bool,
}

/// Sampled health figures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthState {
  /// how incoming searches are being answered
  pub search: SearchHealth,
}

/// Health of the search side.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchHealth {
  /// incoming search requests from other users
  pub incoming: IncomingSearchHealth,
}

/// How incoming search requests are being answered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IncomingSearchHealth {
  /// the moving average time to answer one, in milliseconds
  pub latency: f64,
  /// how many are waiting to be answered
  pub queue_depth: u32,
  /// how many were dropped in the current window
  pub drop_rate: f64,
}

/// Our own Soulseek account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserState {
  /// our username while connected
  pub username: Option<String>,
  /// whether we are online or away, as others see us while we are logged in
  pub status: OwnStatus,
  /// our privileges as the server reports them
  pub privileges: Privileges,
  /// our statistics as the server reports them
  pub statistics: UserStatistics,
}

/// Whether we are at the keyboard, as other users see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum OwnStatus {
  /// at the keyboard
  Online,
  /// away: set by hand, or after nobody had the UI open for a while
  Away,
}

/// Privileges on the Soulseek network.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Privileges {
  /// `privilegesRemaining` is above zero
  pub is_privileged: bool,
  /// seconds of privilege left
  pub privileges_remaining: u32,
  /// when the privileges run out, null without any
  pub privileged_until: Option<DateTime<Utc>>,
}

/// A user's statistics as the Soulseek server keeps them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserStatistics {
  /// average upload speed in bytes per second
  pub average_speed: u32,
  /// shared directories
  pub directory_count: u32,
  /// shared files
  pub file_count: u32,
  /// uploads made
  pub upload_count: u64,
}

/// Our place in the distributed search network.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DistributedNetworkState {
  /// our level in the tree, 0 at a branch root
  pub branch_level: i32,
  /// the username at the root of our branch
  pub branch_root: Option<String>,
  /// whether we take children
  pub can_accept_children: bool,
  /// how many children we take at most
  pub child_limit: u32,
  /// the usernames of our children
  pub children: Vec<String>,
  /// whether we have a parent
  pub has_parent: bool,
  /// whether we are a branch root
  pub is_branch_root: bool,
  /// our parent's username
  pub parent: Option<String>,
}

/// The share scan and the share cache.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SharesState {
  /// the shared directories or filters changed and a scan is needed for it to take effect
  pub scan_pending: bool,
  /// a scan is running
  pub scanning: bool,
  /// the share cache can answer requests
  pub ready: bool,
  /// the last scan failed
  pub faulted: bool,
  /// the last scan was canceled
  pub cancelled: bool,
  /// how far the running scan is, from 0 to 1
  pub scan_progress: f64,
  /// the share hosts, `local` and any relay agents; null until the shares were first loaded
  pub hosts: Option<Vec<String>>,
  /// shared directories over every host
  pub directories: u32,
  /// shared files over every host
  pub files: u32,
}

/// The directories downloads are kept in, as absolute paths on the daemon's machine, so a
/// download's `localPath` can be told relative to its root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectoriesState {
  /// finished downloads, the `downloads` root of the files endpoints
  #[schema(example = "/var/lib/reseek/downloads")]
  pub downloads: String,
  /// downloads in progress, the `incomplete` root
  #[schema(example = "/var/lib/reseek/incomplete")]
  pub incomplete: String,
}

/// The outcome of the last configuration reload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationState {
  /// why the last reload failed, null once a reload succeeds; the values from before stay in
  /// effect
  pub last_reload_error: Option<String>,
  /// when that reload failed, null once a reload succeeds
  pub last_reload_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn server_state_flags_are_written_by_name() {
    let state = ServerConnectionState::Connected | ServerConnectionState::LoggedIn;
    assert_eq!(
      serde_json::to_string(&state).unwrap(),
      "\"Connected, LoggedIn\""
    );
    assert_eq!(
      serde_json::to_string(&ServerConnectionState::empty()).unwrap(),
      "\"None\""
    );
    let read: ServerConnectionState = serde_json::from_str("\"Connected, LoggingIn\"").unwrap();
    assert_eq!(
      read,
      ServerConnectionState::Connected | ServerConnectionState::LoggingIn
    );
  }
}
