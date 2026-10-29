//! What can go wrong loading, reloading, overriding or writing a configuration.

use std::path::PathBuf;

use crate::Key;

/// A value the application's validation refused, with the key it belongs to.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{key}: {message}")]
pub struct Invalid {
  /// the key the problem is about
  pub key: Key,
  /// what is wrong with its value
  pub message: String,
}

impl Invalid {
  /// A problem with the value of `key`.
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
    /// the file
    path: PathBuf,
    /// why
    #[source]
    source: std::io::Error,
  },
  /// a file is not valid TOML
  #[error("{file}: {message}")]
  Parse {
    /// the file's name
    file: String,
    /// where and what, on one line
    message: String,
  },
  /// a value has the wrong type or shape for its key
  #[error("{key} (set by {by}): {message}")]
  Value {
    /// the key
    key: Key,
    /// what set the value: a file, a variable, a flag or an override
    by: String,
    /// what is wrong with it
    message: String,
  },
  /// the application's validation refused the configuration
  #[error("invalid configuration: {}", join(.0))]
  Invalid(Vec<Invalid>),
  /// the stored runtime overrides could not be read
  #[error("{}: {message}", path.display())]
  Overrides {
    /// the overrides file
    path: PathBuf,
    /// what is wrong with it
    message: String,
  },
  /// the file changed since the revision an edit was based on
  #[error("config file changed on disk")]
  Stale,
  /// the file can not be written
  #[error("{file} is read-only: {reason}")]
  ReadOnly {
    /// the file's name
    file: String,
    /// why it can not be written
    reason: String,
  },
  /// the key's value comes from an env var or a flag, which nothing at run time changes, or from
  /// a later file, which the change would not beat
  #[error("{key} is set by {by}")]
  SetBy {
    /// the key
    key: Key,
    /// the variable, flag or file
    by: String,
  },
  /// a key that no field of the configuration has
  #[error("unknown key {0}")]
  UnknownKey(Key),
  /// a runtime override was asked for a key that only takes effect on restart
  #[error("{0} needs a restart; write it to the file")]
  NeedsRestart(Key),
  /// the key has no runtime override to remove
  #[error("no runtime override for {0}")]
  NoOverride(Key),
  /// a write named a file the configuration is not loaded from
  #[error("{0} is not one of the configuration files")]
  UnknownFile(String),
  /// a file could not be written
  #[error("writing {}: {source}", path.display())]
  Write {
    /// the file
    path: PathBuf,
    /// why
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
