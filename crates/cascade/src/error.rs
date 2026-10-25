//! What can go wrong loading, reloading, overriding or writing a configuration.

use std::path::PathBuf;

use crate::Key;

/// A value the application's validation refused, with the key it belongs to.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{key}: {message}")]
pub struct Invalid {
  pub key: Key,
  pub message: String,
}

impl Invalid {
  pub fn new(key: impl Into<Key>, message: impl Into<String>) -> Self {
    Self {
      key: key.into(),
      message: message.into(),
    }
  }
}

/// An error of the configuration library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// a file could not be read
  #[error("reading {}: {source}", path.display())]
  Read {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  /// a file is not valid TOML
  #[error("{file}: {message}")]
  Parse { file: String, message: String },
  /// a value has the wrong type or shape for its key; `by` is what set it
  #[error("{key} (set by {by}): {message}")]
  Value {
    key: Key,
    by: String,
    message: String,
  },
  /// the application's validation refused the configuration
  #[error("invalid configuration: {}", join(.0))]
  Invalid(Vec<Invalid>),
  /// the stored runtime overrides could not be read
  #[error("{}: {message}", path.display())]
  Overrides { path: PathBuf, message: String },
  /// the file changed since the revision an edit was based on
  #[error("config file changed on disk")]
  Stale,
  /// the file can not be written, and why
  #[error("{file} is read-only: {reason}")]
  ReadOnly { file: String, reason: String },
  /// the key's value comes from an env var or flag, which a file or override would not beat
  #[error("{key} is set by {by}")]
  SetBy { key: Key, by: String },
  #[error("unknown key {0}")]
  UnknownKey(Key),
  /// a runtime override was asked for a key that only takes effect on restart
  #[error("{0} needs a restart; write it to the file")]
  NeedsRestart(Key),
  #[error("no runtime override for {0}")]
  NoOverride(Key),
  /// a write named a file the configuration is not loaded from
  #[error("{0} is not one of the configuration files")]
  UnknownFile(String),
  #[error("writing {}: {source}", path.display())]
  Write {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  /// the runtime is shutting down, so the work could not run
  #[error("the configuration watcher has stopped")]
  Stopped,
}

fn join(invalid: &[Invalid]) -> String {
  invalid
    .iter()
    .map(Invalid::to_string)
    .collect::<Vec<_>>()
    .join("; ")
}
