//! The layer that set a key's value.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// The layer whose value of a key is in effect, with what in that layer set it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(tag = "layer", rename_all = "lowercase")]
pub enum Source {
  /// no layer sets the key
  Default,
  /// a configuration file
  File {
    /// its name, such as `config.toml` or `config.d/10-shares.toml`
    file: String,
  },
  /// an environment variable
  Env {
    /// its name
    var: String,
  },
  /// a command-line flag
  Cli {
    /// its long form, such as `--debug`
    flag: String,
  },
  /// an override set at run time
  #[serde(rename_all = "camelCase")]
  Runtime {
    /// when the override ends on its own, `None` for never
    expires_at: Option<DateTime<Utc>>,
    /// the override ends with the process
    until_restart: bool,
    /// who set it
    set_by: String,
    /// when it was set
    set_at: DateTime<Utc>,
  },
}

impl Source {
  /// The variable or flag when the value comes from the environment or the command line, which
  /// nothing at run time can change.
  pub fn fixed_by(&self) -> Option<&str> {
    match self {
      Self::Env { var } => Some(var),
      Self::Cli { flag } => Some(flag),
      _ => None,
    }
  }

  /// What set the value, for messages: a file name, a variable, a flag, `the defaults` or
  /// `a runtime override`.
  pub(crate) fn describe(&self) -> &str {
    match self {
      Self::Default => "the defaults",
      Self::File { file } => file,
      Self::Env { var } => var,
      Self::Cli { flag } => flag,
      Self::Runtime { .. } => "a runtime override",
    }
  }
}
