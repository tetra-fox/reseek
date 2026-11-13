//! Every option, as one struct tree whose field paths are the configuration keys
//! (`web.https.port`). The field's doc comment is the option's help text; its `cascade`
//! attribute names the environment variable, the command-line flag, when a change takes effect
//! and whether the value is secret.

mod analysis;
mod authentication;
mod flags;
mod logging;
mod notifications;
mod paths;
mod relay;
mod retention;
mod schedules;
mod search;
mod soulseek;
mod transfers;
mod web;

use std::collections::BTreeMap;
use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

pub use analysis::{Analysis, Spectrogram, Workers, processors};
pub use authentication::{
  ApiKey, Authentication, Local, LocalUser, Oidc, OidcLogout, OidcRoleMapping, Proxy, RoleMapping,
};
pub use flags::Flags;
pub use logging::{Logger, Logging, Metrics, MetricsAuthentication};
pub use notifications::{
  BuddyNotifications, ChatNotifications, JobNotifications, NotificationMode, Notifications,
  SearchNotifications, TransferNotifications,
};
pub use paths::{Directories, Paths};
pub use relay::{Relay, RelayAgent, RelayLimits, RelayTimeouts};
pub use reseek_shares::options::{MAX_SCAN_WORKERS, ShareCache, Shares};
pub use retention::{
  FileRetention, JobRetention, MessageRetention, Retention, TransferRetention, TransfersRetention,
};
pub use schedules::Schedules;
pub use search::{FilterPreset, Search};
pub use soulseek::{
  ConnectionBuffers, ConnectionTimeouts, DistributedNetwork, ServerKeepalive, Soulseek,
  SoulseekConnection, SoulseekInterests, SoulseekLimits, SoulseekProxy,
};
pub use transfers::{
  BUDDIES_GROUP, BUILTIN_GROUPS, Blacklisted, Buddies, DEFAULT_GROUP, Destination, Download, Group,
  GroupUpload, Groups, LEECHERS_GROUP, LeecherThresholds, Leechers, Limit, Limits,
  PRIVILEGED_GROUP, Permissions, Received, Retry, Transfers, TransfersUi, Upload, UserDefinedGroup,
  is_group_name,
};
pub use web::{Certificate, Cors, Https, Live, Web, WebFiles};

/// The whole configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Options {
  /// Logs at debug level.
  #[cascade(env = "RESEEK_DEBUG", cli = "-d|--debug", apply = "restart")]
  pub debug: bool,
  /// Serves the API only, without the web UI.
  #[cascade(env = "RESEEK_HEADLESS", cli = "-H|--headless", apply = "restart")]
  pub headless: bool,
  /// Lets administrators change settings from the web UI and the API.
  #[cascade(env = "RESEEK_REMOTE_CONFIGURATION", cli = "--remote-configuration")]
  pub remote_configuration: bool,
  /// Allows deleting files in the download directories from the web UI and the API.
  #[cascade(
    env = "RESEEK_REMOTE_FILE_MANAGEMENT",
    cli = "--remote-file-management"
  )]
  pub remote_file_management: bool,
  /// Name of this instance in logs.
  #[cascade(
    env = "RESEEK_INSTANCE_NAME",
    cli = "-i|--instance-name",
    apply = "restart"
  )]
  pub instance_name: String,
  /// How long a shutdown waits for open HTTP connections to finish. Transfers in progress are not
  /// waited for.
  #[cascade(
    env = "RESEEK_SHUTDOWN_TIMEOUT",
    cli = "--shutdown-timeout",
    apply = "restart"
  )]
  #[serde(with = "crate::config::duration")]
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
  /// How often the recurring work runs.
  pub schedules: Schedules,
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
      remote_configuration: true,
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
      schedules: Schedules::default(),
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
  /// Refuses users at the addresses in the blocklist file.
  #[cascade(env = "RESEEK_BLACKLIST", cli = "--enable-blacklist")]
  pub enabled: bool,
  /// Blocklist file in CIDR, P2P or DAT format, read again when a blocklist setting changes. Listed
  /// users also have their private messages dropped and room messages hidden, once their address is
  /// known.
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
  /// Number of incoming searches answered at once. Others wait in the search queue.
  #[cascade(
    env = "RESEEK_THROTTLING_SEARCH_INCOMING_CONCURRENCY",
    cli = "--throttling-search-incoming-concurrency",
    apply = "restart"
  )]
  pub concurrency: u32,
  /// Maximum number of incoming searches waiting to be answered. Searches past it are dropped.
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
  /// Programs run when events happen, by name.
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
  #[serde(with = "crate::config::duration")]
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
  /// Lets other users add you to their private rooms.
  #[cascade(env = "RESEEK_CHAT_ACCEPT_PRIVATE_ROOM_INVITATIONS")]
  pub accept_private_room_invitations: bool,
  /// Words and phrases that mark a room message as a mention. Matched as whole words, in any
  /// case.
  #[cascade(env = "RESEEK_CHAT_HIGHLIGHT_WORDS")]
  pub highlight_words: Vec<String>,
  /// Time with the web UI closed before you are shown as away. `0` turns this off.
  #[cascade(env = "RESEEK_CHAT_AUTO_AWAY")]
  #[serde(with = "crate::config::duration")]
  pub auto_away: Duration,
  /// Sent while you are away, as an automatic reply to each user's first private message.
  #[cascade(env = "RESEEK_CHAT_AUTO_REPLY")]
  pub auto_reply: Option<String>,
}

impl Default for Chat {
  fn default() -> Self {
    Self {
      notifications: ChatNotifications::default(),
      max_message_length: 2000,
      accept_private_room_invitations: true,
      highlight_words: Vec::new(),
      auto_away: Duration::from_secs(15 * 60),
      auto_reply: None,
    }
  }
}

/// User blocking.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Blocking {
  /// What a new block covers unless it says otherwise. Any of `uploads`, `searches`, `browse`,
  /// `messages`, `rooms`, `downloads` and `distributed`.
  #[cascade(env = "RESEEK_BLOCKING_DEFAULT_SCOPES")]
  pub default_scopes: Vec<String>,
  /// What a user blocked from uploads is told when they try to download from you.
  #[cascade(env = "RESEEK_BLOCKING_REFUSAL_MESSAGE")]
  pub refusal_message: String,
}

impl Default for Blocking {
  fn default() -> Self {
    Self {
      default_scopes: api_types::users::blocking::BlockScope::DEFAULT
        .iter()
        .map(ToString::to_string)
        .collect(),
      refusal_message: crate::transfers::uploads::NOT_SHARED.to_string(),
    }
  }
}
