//! Every option, as one struct tree whose field paths are the configuration keys
//! (`web.https.port`). The field's doc comment is the option's help text; its `cascade`
//! attribute names the environment variable, the command-line flag, when a change takes effect
//! and whether the value is secret.

mod analysis;
mod authentication;
mod flags;
mod logging;
mod paths;
mod relay;
mod retention;
mod search;
mod shares;
mod soulseek;
mod transfers;
mod web;

use std::collections::BTreeMap;
use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

pub use analysis::{Analysis, MAX_ANALYSIS_WORKERS, Spectrogram};
pub use authentication::{
  ApiKey, Authentication, Local, LocalUser, Oidc, OidcLogout, OidcRoleMapping, Proxy, RoleMapping,
};
pub use flags::Flags;
pub use logging::{Logger, Logging, Metrics, MetricsAuthentication};
pub use paths::{Directories, Paths};
pub use relay::{Relay, RelayAgent, RelayController, RelayLimits, RelayTimeouts};
pub use retention::{
  FileRetention, MessageRetention, RETENTION_SWEEP, Retention, TransferRetention,
  TransfersRetention,
};
pub use search::{FilterPreset, Search};
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
  /// Debug mode. Logs at debug level and includes the module name on every log line.
  #[cascade(env = "RESEEK_DEBUG", cli = "-d|--debug", apply = "restart")]
  pub debug: bool,
  /// Serves the API only, without the web UI.
  #[cascade(env = "RESEEK_HEADLESS", cli = "-H|--headless", apply = "restart")]
  pub headless: bool,
  /// Allows administrators to edit the configuration files and runtime overrides through the
  /// API.
  #[cascade(env = "RESEEK_REMOTE_CONFIGURATION", cli = "--remote-configuration")]
  pub remote_configuration: bool,
  /// Allows the files API to delete files in the download and incomplete directories.
  #[cascade(
    env = "RESEEK_REMOTE_FILE_MANAGEMENT",
    cli = "--remote-file-management"
  )]
  pub remote_file_management: bool,
  /// Name of this instance in logs and to a relay controller.
  #[cascade(
    env = "RESEEK_INSTANCE_NAME",
    cli = "-i|--instance-name",
    apply = "restart"
  )]
  pub instance_name: String,
  /// Maximum time a shutdown waits for work in progress before the process exits.
  #[cascade(env = "RESEEK_SHUTDOWN_TIMEOUT", cli = "--shutdown-timeout")]
  #[serde(with = "humantime_serde")]
  pub shutdown_timeout: Duration,
  /// Operational and development switches.
  #[cascade(apply = "restart")]
  pub flags: Flags,
  /// Relay mode.
  pub relay: Relay,
  /// Directories for configuration, data, cache and logs.
  #[cascade(apply = "restart")]
  pub paths: Paths,
  /// Download directories.
  #[cascade(apply = "restart")]
  pub directories: Directories,
  /// Shared directories.
  pub shares: Shares,
  /// Uploads, downloads and upload groups.
  pub transfers: Transfers,
  /// Address blocklist.
  pub blacklist: Blacklist,
  /// Filters on requests from other users.
  pub filters: Filters,
  /// Searches made from the UI.
  pub search: Search,
  /// Chat rooms to join after login. Removing a room from the list does not leave it.
  #[cascade(env = "RESEEK_ROOMS", cli = "--rooms")]
  pub rooms: Vec<String>,
  /// HTTP server and access control.
  pub web: Web,
  /// Retention of finished items.
  pub retention: Retention,
  /// Limits on answering other users.
  pub throttling: Throttling,
  /// Log outputs.
  #[cascade(apply = "restart")]
  pub logger: Logger,
  /// Log levels.
  pub logging: Logging,
  /// Prometheus metrics endpoint.
  #[cascade(apply = "restart")]
  pub metrics: Metrics,
  /// Optional features.
  #[cascade(apply = "restart")]
  pub feature: Feature,
  /// Soulseek network.
  pub soulseek: Soulseek,
  /// Scripts run on events.
  pub integrations: Integrations,
  /// Directory of known users.
  pub users: Users,
  /// Chat rooms and private messages.
  pub chat: Chat,
  /// Notifications other than chat.
  pub notifications: Notifications,
  /// User blocking.
  pub blocking: Blocking,
  /// Audio analysis of downloaded files.
  pub analysis: Analysis,
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
      search: Search::default(),
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
      notifications: Notifications::default(),
      blocking: Blocking::default(),
      analysis: Analysis::default(),
    }
  }
}

/// Address blocklist.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Blacklist {
  /// Enables the address blocklist. Peers at listed addresses are refused.
  #[cascade(
    env = "RESEEK_BLACKLIST",
    cli = "--enable-blacklist",
    apply = "restart"
  )]
  pub enabled: bool,
  /// Blocklist file in CIDR, P2P or DAT format. Reloaded when a `blacklist` option changes.
  #[cascade(env = "RESEEK_BLACKLIST_FILE", cli = "--blacklist-file")]
  pub file: Option<std::path::PathBuf>,
}

/// Filters on requests from other users.
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
  /// Regular expressions. Incoming searches that match any of them are not answered.
  #[cascade(env = "RESEEK_SEARCH_REQUEST_FILTER", cli = "--search-request-filter")]
  pub request: Vec<String>,
}

/// Limits on answering searches from other users.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Throttling {
  /// Incoming searches.
  pub search: SearchThrottling,
}

/// Limits on answering searches from other users.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct SearchThrottling {
  /// Incoming searches.
  pub incoming: IncomingSearchThrottling,
}

/// Limits on answering searches from other users.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct IncomingSearchThrottling {
  /// Maximum number of incoming searches answered at once.
  #[cascade(
    env = "RESEEK_THROTTLING_SEARCH_INCOMING_CONCURRENCY",
    cli = "--throttling-search-incoming-concurrency",
    apply = "restart"
  )]
  pub concurrency: u32,
  /// Maximum number of incoming searches waiting to be answered. Further searches are
  /// dropped and counted as dropped searches.
  #[cascade(
    env = "RESEEK_THROTTLING_SEARCH_INCOMING_CIRCUIT_BREAKER",
    cli = "--throttling-search-incoming-circuit-breaker"
  )]
  pub circuit_breaker: u32,
  /// Maximum number of files in one search response.
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

/// Optional features.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Feature {
  /// Serves the OpenAPI document at `/swagger/v0/swagger.json` and the API browser at
  /// `/swagger`.
  #[cascade(env = "RESEEK_SWAGGER", cli = "--swagger")]
  pub swagger: bool,
}

/// Scripts run on events.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Integrations {
  /// Scripts by name, each run on its events.
  #[cascade(user_keyed)]
  pub scripts: BTreeMap<String, Script>,
}

/// A program run on events. The event is passed as JSON in `RESEEK_SCRIPT_DATA`, and the
/// outcome of a download in `RESEEK_SCRIPT_RESULT` and the other `RESEEK_SCRIPT_*` variables.
/// These variables contain text chosen by other users, such as filenames, usernames and
/// messages. Quote them, and never place one unquoted in a shell command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Script {
  /// Names of the events that run the script. `Any`, `All` or `*` matches every event.
  pub on: Vec<String>,
  /// Command the script runs.
  pub run: ScriptRun,
  /// Maximum run time before the script is stopped. `0` for no limit.
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

/// Command a script runs: a shell command, or a program with its arguments.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ScriptRun {
  /// Command line run by the system shell.
  pub command: Option<String>,
  /// Program started directly, with `args` or `arglist`.
  pub executable: Option<String>,
  /// Program arguments as one string, quoted by the user.
  pub args: Option<String>,
  /// Program arguments as a list, each passed unchanged. Safer than `args`, where quoting
  /// determines where each argument ends.
  pub arglist: Option<Vec<String>>,
}

/// Directory of known users.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Users {
  /// Stores the profile pictures of other users.
  #[cascade(env = "RESEEK_USERS_CACHE_PICTURES")]
  pub cache_pictures: bool,
  /// Largest profile picture stored.
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

/// Chat rooms and private messages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Chat {
  /// Browser notifications for chat.
  pub notifications: ChatNotifications,
  /// Maximum length of a sent message, in characters.
  #[cascade(env = "RESEEK_CHAT_MAX_MESSAGE_LENGTH")]
  pub max_message_length: u32,
  /// Accepts invitations to private rooms. When off, the server is told that invitations are
  /// not accepted.
  #[cascade(env = "RESEEK_CHAT_ACCEPT_PRIVATE_ROOM_INVITATIONS")]
  pub accept_private_room_invitations: bool,
  /// Words and phrases that mark a room message as a mention. Matched as whole words, in any
  /// case.
  #[cascade(env = "RESEEK_CHAT_HIGHLIGHT_WORDS")]
  pub highlight_words: Vec<String>,
}

impl Default for Chat {
  fn default() -> Self {
    Self {
      notifications: ChatNotifications::default(),
      max_message_length: 2000,
      accept_private_room_invitations: true,
      highlight_words: Vec::new(),
    }
  }
}

/// Browser notifications for chat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct ChatNotifications {
  /// Enables chat notifications, once the browser permits them.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_ENABLED")]
  pub enabled: bool,
  /// Suppresses notifications while the tab is visible with the chat of the message open.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_ONLY_WHEN_HIDDEN")]
  pub only_when_hidden: bool,
  /// Plays a sound with each notification.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_SOUND")]
  pub sound: bool,
  /// Notifies of private messages.
  #[cascade(env = "RESEEK_CHAT_NOTIFICATIONS_PRIVATE_MESSAGES")]
  pub private_messages: bool,
  /// Notifies of room messages that mention this account.
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

/// Notifications other than chat.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Notifications {
  /// Transfer notifications.
  pub transfers: TransferNotifications,
}

/// Transfer notifications, under the same browser permission and tab visibility rules as chat
/// notifications.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct TransferNotifications {
  /// Notifies when a download finishes.
  #[cascade(env = "RESEEK_NOTIFICATIONS_TRANSFERS_DOWNLOAD_FINISHED")]
  pub download_finished: bool,
  /// Notifies when every download of a batch, such as an album, has finished.
  #[cascade(env = "RESEEK_NOTIFICATIONS_TRANSFERS_BATCH_FINISHED")]
  pub batch_finished: bool,
  /// Notifies when another user queues an upload.
  #[cascade(env = "RESEEK_NOTIFICATIONS_TRANSFERS_UPLOAD_QUEUED")]
  pub upload_queued: bool,
}

impl Default for TransferNotifications {
  fn default() -> Self {
    Self {
      download_finished: true,
      batch_finished: true,
      upload_queued: false,
    }
  }
}

/// User blocking.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Blocking {
  /// Scopes of a new block unless it specifies its own. Any of `uploads`, `searches`,
  /// `browse`, `messages`, `rooms`, `downloads` and `distributed`.
  #[cascade(env = "RESEEK_BLOCKING_DEFAULT_SCOPES")]
  pub default_scopes: Vec<String>,
}

impl Default for Blocking {
  fn default() -> Self {
    Self {
      default_scopes: api_types::users::blocking::BlockScope::DEFAULT
        .iter()
        .map(ToString::to_string)
        .collect(),
    }
  }
}
