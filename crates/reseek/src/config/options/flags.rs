//! Operational and development switches, all read once at startup.

use cascade::Cascade;
use serde::{Deserialize, Serialize};

/// Operational and development switches, all off by default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Flags {
  /// Leaves out the startup banner.
  #[cascade(env = "RESEEK_NO_LOGO", cli = "-n|--no-logo")]
  pub no_logo: bool,
  /// Sets everything up, then exits instead of serving; for checking a configuration.
  #[cascade(env = "RESEEK_NO_START", cli = "-x|--no-start")]
  pub no_start: bool,
  /// Does not watch the configuration files; changes then need a reload signal or a restart.
  #[cascade(env = "RESEEK_NO_CONFIG_WATCH", cli = "--no-config-watch")]
  pub no_config_watch: bool,
  /// Watches the configuration files by reading them every few seconds, for mounts where file
  /// notifications do not arrive.
  #[cascade(env = "RESEEK_CONFIG_WATCH_POLL", cli = "--config-watch-poll")]
  pub config_watch_poll: bool,
  /// Does not connect to the Soulseek server at startup.
  #[cascade(env = "RESEEK_NO_CONNECT", cli = "--no-connect")]
  pub no_connect: bool,
  /// Does not scan the shares at startup.
  #[cascade(env = "RESEEK_NO_SHARE_SCAN", cli = "--no-share-scan")]
  pub no_share_scan: bool,
  /// Scans the shares in full at startup even when the share cache is current.
  #[cascade(env = "RESEEK_FORCE_SHARE_SCAN", cli = "--force-share-scan")]
  pub force_share_scan: bool,
  /// Applies every database migration whether or not it looks applied.
  #[cascade(env = "RESEEK_FORCE_MIGRATIONS", cli = "--force-migrations")]
  pub force_migrations: bool,
  /// Does not check for a newer release.
  #[cascade(env = "RESEEK_NO_VERSION_CHECK", cli = "--no-version-check")]
  pub no_version_check: bool,
  /// Logs every SQL statement with its values; the log then holds usernames, filenames and
  /// messages.
  #[cascade(env = "RESEEK_LOG_SQL", cli = "--log-sql")]
  pub log_sql: bool,
  /// Logs the failure of background work nothing waits for.
  #[cascade(
    env = "RESEEK_LOG_UNOBSERVED_EXCEPTIONS",
    cli = "--log-unobserved-exceptions"
  )]
  pub log_unobserved_exceptions: bool,
  /// Turns on features still being tried out.
  #[cascade(env = "RESEEK_EXPERIMENTAL", cli = "--experimental")]
  pub experimental: bool,
  /// Keeps every database in memory: nothing is saved and nothing survives a restart.
  #[cascade(env = "RESEEK_VOLATILE", cli = "--volatile")]
  pub volatile: bool,
  /// Makes user-defined regular expressions case-sensitive.
  #[cascade(env = "RESEEK_CASE_SENSITIVE_REGEX", cli = "--case-sensitive-regex")]
  pub case_sensitive_regex: bool,
  /// Uses the older socket keepalive control on Windows versions before 10 1709.
  #[cascade(
    env = "RESEEK_LEGACY_WINDOWS_TCP_KEEPALIVE",
    cli = "--legacy-windows-tcp-keepalive"
  )]
  pub legacy_windows_tcp_keepalive: bool,
  /// A relay controller trusts an agent's share listing for whether a file exists, instead of
  /// asking the agent first.
  #[cascade(
    env = "RESEEK_OPTIMISTIC_RELAY_FILE_INFO",
    cli = "--optimistic-relay-file-info"
  )]
  pub optimistic_relay_file_info: bool,
}
