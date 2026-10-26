//! Every option, as one struct tree whose field paths are the configuration keys
//! (`web.https.port`). The field's doc comment is the option's help text; its `cascade`
//! attribute names the environment variable, the command-line flag, when a change takes effect
//! and whether the value is secret.

mod authentication;
mod flags;
mod logging;
mod paths;
mod relay;
mod retention;
mod shares;
mod soulseek;
mod transfers;
mod web;

use std::collections::BTreeMap;
use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

pub use authentication::{
  ApiKey, Authentication, Local, LocalUser, Oidc, OidcLogout, OidcRoleMapping, Proxy, RoleMapping,
};
pub use flags::Flags;
pub use logging::{Logger, Logging, Metrics, MetricsAuthentication};
pub use paths::{Directories, Paths};
pub use relay::{Relay, RelayAgent, RelayController};
pub use retention::{
  FileRetention, MessageRetention, RETENTION_SWEEP, Retention, TransferRetention,
  TransfersRetention,
};
pub use shares::{MAX_SCAN_WORKERS, ShareCache, Shares};
pub use soulseek::{
  ConnectionBuffers, ConnectionTimeouts, DistributedNetwork, ServerKeepalive, Soulseek,
  SoulseekConnection, SoulseekLimits, SoulseekProxy,
};
pub use transfers::{
  Blacklisted, Destination, Download, Group, GroupUpload, Groups, LeecherThresholds, Leechers,
  Limit, Limits, Permissions, Retry, Transfers, TransfersUi, Upload, UserDefinedGroup,
};
pub use web::{Certificate, Cors, Https, Live, Web, WebFiles};

/// The whole configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Options {
  /// Debug mode: logs at debug level and names the module on every log line.
  #[cascade(env = "RESEEK_DEBUG", cli = "-d|--debug", apply = "restart")]
  pub debug: bool,
  /// Serves the API only, no web UI.
  #[cascade(env = "RESEEK_HEADLESS", cli = "-H|--headless", apply = "restart")]
  pub headless: bool,
  /// Lets administrators change the configuration files and runtime overrides through the
  /// API.
  #[cascade(env = "RESEEK_REMOTE_CONFIGURATION", cli = "--remote-configuration")]
  pub remote_configuration: bool,
  /// Lets the files API delete files in the download and incomplete directories.
  #[cascade(
    env = "RESEEK_REMOTE_FILE_MANAGEMENT",
    cli = "--remote-file-management"
  )]
  pub remote_file_management: bool,
  /// Names this instance in logs and to a relay controller.
  #[cascade(
    env = "RESEEK_INSTANCE_NAME",
    cli = "-i|--instance-name",
    apply = "restart"
  )]
  pub instance_name: String,
  /// How long a shutdown waits for work in progress before the process exits anyway.
  #[cascade(env = "RESEEK_SHUTDOWN_TIMEOUT", cli = "--shutdown-timeout")]
  #[serde(with = "humantime_serde")]
  pub shutdown_timeout: Duration,
  /// Operational and development switches.
  #[cascade(apply = "restart")]
  pub flags: Flags,
  /// Relay mode.
  pub relay: Relay,
  /// Where reseek keeps its files.
  #[cascade(apply = "restart")]
  pub paths: Paths,
  /// Where downloads are written.
  #[cascade(apply = "restart")]
  pub directories: Directories,
  /// What we share.
  pub shares: Shares,
  /// Uploads, downloads and the groups uploads are queued by.
  pub transfers: Transfers,
  /// The managed blocklist of addresses.
  pub blacklist: Blacklist,
  /// Filters on what other users send us.
  pub filters: Filters,
  /// Chat rooms to join after logging in. Rooms taken off the list are not left.
  #[cascade(env = "RESEEK_ROOMS", cli = "--rooms")]
  pub rooms: Vec<String>,
  /// The HTTP server and who may use it.
  pub web: Web,
  /// How long finished things are kept.
  pub retention: Retention,
  /// Limits on answering other users.
  pub throttling: Throttling,
  /// The log sinks.
  #[cascade(apply = "restart")]
  pub logger: Logger,
  /// What is logged.
  pub logging: Logging,
  /// The Prometheus metrics endpoint.
  #[cascade(apply = "restart")]
  pub metrics: Metrics,
  /// Features that are off unless asked for.
  #[cascade(apply = "restart")]
  pub feature: Feature,
  /// The Soulseek network.
  pub soulseek: Soulseek,
  /// The user's own programs, run on events.
  pub integrations: Integrations,
  /// The directory of users we have seen.
  pub users: Users,
  /// Chat in rooms and private messages.
  pub chat: Chat,
  /// Blocking users.
  pub blocking: Blocking,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      debug: false,
      headless: false,
      remote_configuration: false,
      remote_file_management: false,
      instance_name: "default".to_string(),
      shutdown_timeout: Duration::from_secs(10),
      flags: Flags::default(),
      relay: Relay::default(),
      paths: Paths::default(),
      directories: Directories::default(),
      shares: Shares::default(),
      transfers: Transfers::default(),
      blacklist: Blacklist::default(),
      filters: Filters::default(),
      rooms: Vec::new(),
      web: Web::default(),
      retention: Retention::default(),
      throttling: Throttling::default(),
      logger: Logger::default(),
      logging: Logging::default(),
      metrics: Metrics::default(),
      feature: Feature::default(),
      soulseek: Soulseek::default(),
      integrations: Integrations::default(),
      users: Users::default(),
      chat: Chat::default(),
      blocking: Blocking::default(),
    }
  }
}

/// A managed blocklist of network addresses.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Blacklist {
  /// Loads the blocklist file and refuses peers whose address it lists.
  #[cascade(
    env = "RESEEK_BLACKLIST",
    cli = "--enable-blacklist",
    apply = "restart"
  )]
  pub enabled: bool,
  /// The blocklist file, in CIDR, P2P or DAT format; read again when a `blacklist` option
  /// changes.
  #[cascade(env = "RESEEK_BLACKLIST_FILE", cli = "--blacklist-file")]
  pub file: Option<std::path::PathBuf>,
}

/// Filters on what other users send us.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Filters {
  /// Filters on incoming searches.
  pub search: SearchFilters,
}

/// Filters on incoming searches.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SearchFilters {
  /// Regular expressions; an incoming search whose text matches one is not answered.
  #[cascade(env = "RESEEK_SEARCH_REQUEST_FILTER", cli = "--search-request-filter")]
  pub request: Vec<String>,
}

/// Limits on answering other users' searches.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Throttling {
  /// Incoming searches.
  pub search: SearchThrottling,
}

/// Limits on answering other users' searches.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SearchThrottling {
  /// Incoming searches.
  pub incoming: IncomingSearchThrottling,
}

/// Limits on answering other users' searches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct IncomingSearchThrottling {
  /// How many incoming searches are answered at the same time.
  #[cascade(
    env = "RESEEK_THROTTLING_SEARCH_INCOMING_CONCURRENCY",
    cli = "--throttling-search-incoming-concurrency",
    apply = "restart"
  )]
  pub concurrency: u32,
  /// How many incoming searches may wait before new ones are dropped.
  #[cascade(
    env = "RESEEK_THROTTLING_SEARCH_INCOMING_CIRCUIT_BREAKER",
    cli = "--throttling-search-incoming-circuit-breaker"
  )]
  pub circuit_breaker: u32,
  /// The most files one search response carries.
  #[cascade(
    env = "RESEEK_THROTTLING_SEARCH_INCOMING_RESPONSE_FILE_LIMIT",
    cli = "--throttling-search-incoming-response-file-limit"
  )]
  pub response_file_limit: u32,
}

impl Default for IncomingSearchThrottling {
  fn default() -> Self {
    Self {
      concurrency: 10,
      circuit_breaker: 500,
      response_file_limit: 500,
    }
  }
}

/// Features that are off unless asked for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Feature {
  /// Serves the OpenAPI document at `/swagger/v0/swagger.json` and its browser at `/swagger`.
  #[cascade(env = "RESEEK_SWAGGER", cli = "--swagger")]
  pub swagger: bool,
}

/// The user's own programs, run on events.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Integrations {
  /// Scripts by name, each run when one of its events is raised.
  #[cascade(user_keyed)]
  pub scripts: BTreeMap<String, Script>,
}

/// A program run when one of its events is raised.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Script {
  /// The events that run it, by name; `Any` for every event.
  pub on: Vec<String>,
  /// What the script runs.
  pub run: ScriptRun,
  /// How long a run may last before it is killed, `0` for no limit.
  #[serde(with = "humantime_serde")]
  pub timeout: Duration,
}

impl Default for Script {
  fn default() -> Self {
    Self {
      on: Vec::new(),
      run: ScriptRun::default(),
      timeout: Duration::from_secs(60),
    }
  }
}

/// What a script runs: a shell command, or a program with its arguments.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ScriptRun {
  /// A command line run by the system shell.
  pub command: Option<String>,
  /// A program started directly, with `args` or `arglist`.
  pub executable: Option<String>,
  /// The program's arguments as one string, quoted by the user.
  pub args: Option<String>,
  /// The program's arguments as a list, each passed as it is.
  pub arglist: Option<Vec<String>>,
}

/// The directory of users we have seen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Users {
  /// Keeps users' profile pictures.
  #[cascade(env = "RESEEK_USERS_CACHE_PICTURES")]
  pub cache_pictures: bool,
  /// The largest profile picture kept.
  #[cascade(env = "RESEEK_USERS_PICTURE_MAX_BYTES", kind = "size")]
  pub picture_max_bytes: bytesize::ByteSize,
}

impl Default for Users {
  fn default() -> Self {
    Self {
      cache_pictures: true,
      picture_max_bytes: bytesize::ByteSize::mb(2),
    }
  }
}

/// Chat in rooms and private messages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Chat {
  /// The browser notifications the UI shows.
  pub notifications: ChatNotifications,
  /// The longest message that may be sent, in characters.
  #[cascade(env = "RESEEK_CHAT_MAX_MESSAGE_LENGTH")]
  pub max_message_length: u32,
}

impl Default for Chat {
  fn default() -> Self {
    Self {
      notifications: ChatNotifications::default(),
      max_message_length: 2000,
    }
  }
}

/// The browser notifications the UI shows for chat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ChatNotifications {
  /// The UI may show notifications, once the browser allows it.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_ENABLED")]
  pub enabled: bool,
  /// No notification while the tab is visible with the message's chat open.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_ONLY_WHEN_HIDDEN")]
  pub only_when_hidden: bool,
  /// Plays a short sound with each notification.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_SOUND")]
  pub sound: bool,
  /// Notifies of private messages.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_PRIVATE_MESSAGES")]
  pub private_messages: bool,
  /// Notifies of room messages that mention us.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_MENTIONS")]
  pub mentions: bool,
  /// Notifies of every message in rooms set to notify of all messages.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_ROOM_MESSAGES")]
  pub room_messages: bool,
}

impl Default for ChatNotifications {
  fn default() -> Self {
    Self {
      enabled: true,
      only_when_hidden: true,
      sound: false,
      private_messages: true,
      mentions: true,
      room_messages: false,
    }
  }
}

/// Blocking users.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Blocking {
  /// What a new block stops unless it says otherwise: any of `uploads`, `searches`, `browse`,
  /// `messages`, `rooms`, `downloads`, `distributed`.
  #[cascade(env = "RESEEK_BLOCKING_DEFAULT_SCOPES")]
  pub default_scopes: Vec<String>,
}

impl Default for Blocking {
  fn default() -> Self {
    Self {
      default_scopes: api_types::blocking::BlockScope::DEFAULT
        .iter()
        .map(ToString::to_string)
        .collect(),
    }
  }
}
