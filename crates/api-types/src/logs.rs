//! Log records as the live log view shows them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// How severe a log record is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum LogLevel {
  /// per-item detail
  Verbose,
  /// the narrative that explains why something happened
  Debug,
  /// a lifecycle event
  Information,
  /// degraded or misconfigured, still running
  Warning,
  /// something an operator must act on
  Error,
  /// the process is ending because of it
  Fatal,
}

/// One log line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
  /// when it was logged
  pub timestamp: DateTime<Utc>,
  /// the module that logged it, such as `reseek::http`
  #[schema(example = "reseek::http")]
  pub context: String,
  /// what the module logged it for, such as a share host or a relay agent, when it says
  pub sub_context: Option<String>,
  /// how severe it is
  pub level: LogLevel,
  /// the message with its fields, and the chain of error messages when an error is attached
  pub message: String,
}
