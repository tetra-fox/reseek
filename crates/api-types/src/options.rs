//! The configuration as the settings page sees it: the values in effect, where each came from,
//! the files, runtime overrides, and the bodies of the calls that change them.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// The options in effect, by their configuration keys, with every secret shown as `*****` and
/// credentials taken out of URLs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[schema(value_type = Object)]
pub struct OptionsDocument(pub Value);

/// The layer whose value of a key is in effect, with what in that layer set it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "layer", rename_all = "lowercase")]
pub enum OptionSource {
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
  /// a runtime override
  #[serde(rename_all = "camelCase")]
  Runtime {
    /// when the override ends on its own, null for never
    expires_at: Option<DateTime<Utc>>,
    /// the override ends when the daemon stops
    until_restart: bool,
    /// who set it
    set_by: String,
    /// when it was set
    set_at: DateTime<Utc>,
  },
}

/// What kind of value a key holds, written as its lowercase name, a list as
/// `{ "list": <kind of each element> }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum OptionKind {
  /// `true` or `false`
  Bool,
  /// a whole number
  Integer,
  /// a number with a fraction
  Float,
  /// text
  String,
  /// a human-readable duration such as `"15m"`
  Duration,
  /// a byte count, an integer or a string such as `"10 MiB"`
  Size,
  /// a list, replaced whole by the layer that sets it
  #[schema(no_recursion)]
  List(Box<OptionKind>),
  /// a table whose keys the user picks, replaced whole by the layer that sets it
  Map,
  /// a table that is an element of a list
  Table,
}

/// When a changed value takes effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Apply {
  /// at once
  Live,
  /// after the next reconnect to the Soulseek server
  Reconnect,
  /// after a restart
  Restart,
}

/// A value the files hold that is not the one in effect, because a reload with it failed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotApplied {
  /// why it is not in effect
  pub reason: String,
  /// the value the failed reload read, `*****` for a secret
  #[schema(value_type = Object)]
  pub value: Value,
}

/// One key: where its value came from and how it can be changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldView {
  /// the layer whose value is in effect
  pub source: OptionSource,
  /// the file a save of the key writes: the last file that sets it, else the first file
  pub file: Option<String>,
  /// the environment variable that sets the key
  #[schema(example = "RESEEK_SLSK_USERNAME")]
  pub env_var: Option<String>,
  /// the long command-line flag that sets the key
  #[schema(example = "--slsk-username")]
  pub cli_flag: Option<String>,
  /// what kind of value the key holds
  pub kind: OptionKind,
  /// the key's help text
  pub doc: String,
  /// the names an option that is one of a fixed set takes, such as the log levels; null for
  /// any other option
  #[schema(example = json!(["error", "warn", "info", "debug", "trace"]))]
  pub choices: Option<Vec<String>>,
  /// the value when nothing sets the key, null for none; `*****` for a secret that has one
  #[schema(value_type = Object)]
  pub default: Value,
  /// when a change to the value takes effect
  pub apply: Apply,
  /// whether the value is shown as `*****`
  pub secret: bool,
  /// the files' value when it is not the one in effect, and why
  pub not_applied: Option<NotApplied>,
  /// the fields of each element of a list of tables or entry of a user-keyed map, so a client
  /// can edit them one by one; empty for any other option
  pub entries: Vec<EntryView>,
}

/// One field of the elements of a list of tables or the entries of a user-keyed map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
  /// the dotted key, relative to the element
  #[schema(example = "role_mapping.claim")]
  pub key: String,
  /// what kind of value the key holds
  pub kind: OptionKind,
  /// the key's help text
  pub doc: String,
  /// the names the value is one of; null for any other field
  pub choices: Option<Vec<String>>,
  /// what a new entry holds until something sets the field, null for nothing, a secret's or
  /// one of a list's elements
  #[schema(value_type = Object)]
  pub default: Value,
  /// whether the value is shown as `*****`
  pub secret: bool,
}

/// One configuration file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
  /// as the files are named, such as `config.toml` or `config.d/10-shares.toml`
  pub name: String,
  /// where the file is
  pub path: String,
  /// a hash of its contents, null when the file does not exist
  #[schema(example = "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")]
  pub revision: Option<String>,
  /// whether a save to the file can be made
  pub writable: bool,
  /// why the file cannot be written
  pub read_only_reason: Option<String>,
}

/// A runtime override in effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OverrideView {
  /// its value, `*****` for a secret
  #[schema(value_type = Object)]
  pub value: Value,
  /// when it ends on its own, null for never
  pub expires_at: Option<DateTime<Utc>>,
  /// it is kept in memory only and ends when the daemon stops
  pub until_restart: bool,
  /// who set it
  pub set_by: String,
  /// when it was set
  pub set_at: DateTime<Utc>,
}

/// The configuration in effect with, for every key, where its value came from and how it can
/// be changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
  /// the values in effect, secrets shown as `*****`
  #[schema(value_type = Object)]
  pub values: Value,
  /// the revision of those values, new each time they change; a change sent with it is refused
  /// with 409 once the values have changed since
  pub revision: String,
  /// every leaf key, by key
  pub fields: BTreeMap<String, FieldView>,
  /// the configuration files, in the order they apply
  pub files: Vec<FileView>,
  /// keys that take effect on restart whose value differs from the one the daemon started
  /// with
  pub pending_restart: Vec<String>,
  /// keys that take effect on reconnect whose value changed since the last connection
  pub pending_reconnect: Vec<String>,
  /// the tool that manages the files, which are then read-only, such as `Nix`
  pub managed_by: Option<String>,
  /// keys, flags and variables that were ignored, each with where it was found
  pub warnings: Vec<String>,
  /// why the last reload failed, null once one succeeds
  pub load_error: Option<String>,
  /// the runtime overrides in effect, by key
  pub overrides: BTreeMap<String, OverrideView>,
}

/// Sets and removes runtime overrides, which apply on top of every other layer until they
/// expire or the files change the value under them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionsPatch {
  /// the keys to override and their values; `*****` for a secret, or for one in an entry of a
  /// map, keeps it
  #[serde(default)]
  #[schema(value_type = Object)]
  pub set: BTreeMap<String, Value>,
  /// the keys whose override ends
  #[serde(default)]
  pub unset: Vec<String>,
  /// how long the overrides set last, such as `"2h"`; `"restart"` for until the daemon stops,
  /// never written to disk; null for until removed
  #[schema(example = "2h")]
  pub expires_in: Option<String>,
  /// the revision of the options the change was made against, from the view; null makes no
  /// check
  #[serde(default)]
  pub revision: Option<String>,
}

/// What a change to the configuration did, by when each changed key takes effect.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionsChanged {
  /// keys whose new value is in effect now
  pub applied: Vec<String>,
  /// keys whose new value takes effect after the next reconnect
  pub reconnect_required: Vec<String>,
  /// keys whose new value takes effect after a restart
  pub restart_required: Vec<String>,
}

/// Changes some keys of a configuration file, keeping its comments.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilePatch {
  /// the keys to write and their values; `*****` for a secret, or for one in an entry of a map,
  /// keeps it, `""` clears it
  #[serde(default)]
  #[schema(value_type = Object)]
  pub set: BTreeMap<String, Value>,
  /// the keys to take out of the file
  #[serde(default)]
  pub unset: Vec<String>,
  /// the file's revision the change was made against, null when the file did not exist
  pub revision: Option<String>,
  /// the revision of the options the change was made against, from the view; null makes no
  /// check
  #[serde(default)]
  pub options_revision: Option<String>,
  /// the file to change, the first configuration file when null
  pub file: Option<String>,
}

/// Replaces the whole text of a configuration file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileReplace {
  /// the new text
  pub text: String,
  /// the file's revision the text was written against, null when the file did not exist
  pub revision: Option<String>,
  /// the file to replace, the first configuration file when null
  pub file: Option<String>,
}

/// What a write to a configuration file did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileWritten {
  /// the file's new revision
  pub revision: String,
  /// what the reload after the write changed
  #[serde(flatten)]
  pub changed: OptionsChanged,
}

/// A configuration file's text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
  /// the file's name, such as `config.toml`
  pub name: String,
  /// where the file is
  pub path: String,
  /// its text, null when the file does not exist
  pub text: Option<String>,
  /// a hash of its contents, null when the file does not exist
  pub revision: Option<String>,
}

/// A text to check as the new contents of a configuration file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileValidate {
  /// the text
  pub text: String,
  /// the file it would replace, the first configuration file when null
  pub file: Option<String>,
}

/// Whether a text would load as the configuration, checked together with every other layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Validation {
  /// whether the configuration would load
  pub valid: bool,
  /// why it would not, empty when it would
  pub errors: Vec<OptionError>,
  /// keys, flags and variables that would be ignored
  pub warnings: Vec<String>,
}

/// One problem with a configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionError {
  /// the key the problem is about, null when it is about the whole text
  pub key: Option<String>,
  /// what is wrong
  pub message: String,
}

/// Which configuration file a call is about.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FileQuery {
  /// the file's name, such as `config.d/10-shares.toml`; the first configuration file when
  /// not given
  pub file: Option<String>,
}
