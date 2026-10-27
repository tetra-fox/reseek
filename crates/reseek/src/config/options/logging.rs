//! The log sinks, the log levels and the metrics endpoint.

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::LogLevel;

/// The log sinks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Logger {
  /// The base URL of a Grafana Loki instance to push logs to.
  #[cascade(env = "RESEEK_LOKI", cli = "--loki")]
  pub loki: Option<String>,
  /// Writes no log files.
  #[cascade(env = "RESEEK_NO_DISK_LOGGER", cli = "--no-disk-logger")]
  pub no_disk: bool,
  /// Writes the console log without colour; `NO_COLOR` set to anything does the same.
  #[cascade(env = "RESEEK_NO_COLOR", cli = "--no-color")]
  pub no_color: bool,
}

/// What is logged; changes take effect at once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Logging {
  /// The least severe level logged: `error`, `warn`, `info`, `debug` or `trace`; `debug` mode
  /// lowers it to `debug`.
  #[cascade(env = "RESEEK_LOG_LEVEL", cli = "--log-level")]
  pub level: LogLevel,
  /// Levels for single modules, such as `soulseek_client::distributed=trace`.
  #[cascade(env = "RESEEK_LOG_TARGETS", cli = "--log-target")]
  pub targets: Vec<String>,
}

impl Default for Logging {
  fn default() -> Self {
    Self {
      level: LogLevel::Info,
      targets: Vec::new(),
    }
  }
}

/// The Prometheus metrics endpoint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Metrics {
  /// Serves Prometheus metrics.
  #[cascade(env = "RESEEK_METRICS", cli = "--metrics")]
  pub enabled: bool,
  /// The path the metrics are served at.
  #[cascade(env = "RESEEK_METRICS_URL", cli = "--metrics-url")]
  pub url: String,
  /// The endpoint's HTTP Basic credentials.
  pub authentication: MetricsAuthentication,
}

impl Default for Metrics {
  fn default() -> Self {
    Self {
      enabled: false,
      url: "/metrics".to_string(),
      authentication: MetricsAuthentication::default(),
    }
  }
}

/// The metrics endpoint's HTTP Basic credentials.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct MetricsAuthentication {
  /// Serves the metrics without credentials.
  #[cascade(env = "RESEEK_METRICS_NO_AUTH", cli = "--metrics-no-auth")]
  pub disabled: bool,
  /// The username.
  #[cascade(env = "RESEEK_METRICS_USERNAME", cli = "--metrics-username")]
  pub username: String,
  /// The password.
  #[cascade(env = "RESEEK_METRICS_PASSWORD", cli = "--metrics-password", secret)]
  pub password: String,
}

impl Default for MetricsAuthentication {
  fn default() -> Self {
    Self {
      disabled: false,
      username: "reseek".to_string(),
      password: "reseek".to_string(),
    }
  }
}
