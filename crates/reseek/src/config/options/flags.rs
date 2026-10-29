//! Operational and development switches.

use cascade::Cascade;
use serde::{Deserialize, Serialize};

/// Operational and development switches, all off by default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Flags {
  /// Omits the startup banner.
  #[cascade(env = "RESEEK_NO_LOGO", cli = "-n|--no-logo")]
  pub no_logo: bool,
  /// Initializes everything, then exits without serving. Used to check a configuration.
  #[cascade(env = "RESEEK_NO_START", cli = "-x|--no-start")]
  pub no_start: bool,
  /// Disables watching the configuration files. Changes then require a reload signal or a
  /// restart.
  #[cascade(env = "RESEEK_NO_CONFIG_WATCH", cli = "--no-config-watch")]
  pub no_config_watch: bool,
  /// Watches the configuration files by reading them every few seconds, for mounts that do
  /// not deliver file notifications.
  #[cascade(env = "RESEEK_CONFIG_WATCH_POLL", cli = "--config-watch-poll")]
  pub config_watch_poll: bool,
  /// Does not connect to the Soulseek server at startup.
  #[cascade(env = "RESEEK_NO_CONNECT", cli = "--no-connect")]
  pub no_connect: bool,
  /// Skips the share scan at startup.
  #[cascade(env = "RESEEK_NO_SHARE_SCAN", cli = "--no-share-scan")]
  pub no_share_scan: bool,
  /// Runs a full share scan at startup, even when the share cache is current.
  #[cascade(env = "RESEEK_FORCE_SHARE_SCAN", cli = "--force-share-scan")]
  pub force_share_scan: bool,
  /// Has no effect, and is accepted for compatibility: migrations apply by version and none runs
  /// twice.
  #[cascade(env = "RESEEK_FORCE_MIGRATIONS", cli = "--force-migrations")]
  pub force_migrations: bool,
  /// Disables the check for a newer release.
  #[cascade(env = "RESEEK_NO_VERSION_CHECK", cli = "--no-version-check")]
  pub no_version_check: bool,
  /// Logs every SQL statement with its values. The log then contains usernames, filenames and
  /// messages.
  #[cascade(env = "RESEEK_LOG_SQL", cli = "--log-sql")]
  pub log_sql: bool,
  /// Logs failures of background work that nothing waits on.
  #[cascade(
    env = "RESEEK_LOG_UNOBSERVED_EXCEPTIONS",
    cli = "--log-unobserved-exceptions"
  )]
  pub log_unobserved_exceptions: bool,
  /// Enables experimental features.
  #[cascade(env = "RESEEK_EXPERIMENTAL", cli = "--experimental")]
  pub experimental: bool,
  /// Keeps every database in memory. Nothing is saved across restarts.
  #[cascade(env = "RESEEK_VOLATILE", cli = "--volatile")]
  pub volatile: bool,
  /// Makes user-defined regular expressions case-sensitive.
  #[cascade(env = "RESEEK_CASE_SENSITIVE_REGEX", cli = "--case-sensitive-regex")]
  pub case_sensitive_regex: bool,
  /// Has no effect, and is accepted for compatibility.
  #[cascade(
    env = "RESEEK_LEGACY_WINDOWS_TCP_KEEPALIVE",
    cli = "--legacy-windows-tcp-keepalive"
  )]
  pub legacy_windows_tcp_keepalive: bool,
  /// Makes a relay controller trust the share listing of an agent for whether a file exists,
  /// instead of asking the agent first.
  #[cascade(
    env = "RESEEK_OPTIMISTIC_RELAY_FILE_INFO",
    cli = "--optimistic-relay-file-info"
  )]
  pub optimistic_relay_file_info: bool,
}
