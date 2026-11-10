//! A loaded configuration with where each value came from.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::Key;
use crate::files::LoadedFile;
use crate::overrides::Override;
use crate::schema::Schema;
use crate::source::Source;

/// A configuration as loaded from its layers.
#[derive(Clone, Debug)]
pub struct Loaded<T> {
  /// the values in effect
  pub values: T,
  /// names these values: a new one each time they change, never the same twice, so a change
  /// made against one is known to be made against values since replaced
  pub revision: String,
  /// the layer that set each leaf key's value
  pub sources: BTreeMap<Key, Source>,
  /// keys no field has, flags no field takes, and variables that lost to another name for the
  /// same key, each with where it was found
  pub warnings: Vec<String>,
  /// why the last reload failed, `None` once one succeeds; the values are the ones from before
  pub load_error: Option<String>,
  /// when the last reload failed
  pub load_error_at: Option<DateTime<Utc>>,
  /// keys whose value in the files is not the one in effect, because a reload with it failed
  pub not_applied: BTreeMap<Key, NotApplied>,
  /// the configuration files, in the order they apply, as they were read
  pub files: Vec<LoadedFile>,
  /// the runtime overrides in effect
  pub overrides: BTreeMap<Key, Override>,
  /// keys that only take effect on restart whose value changed since the start
  pub pending_restart: Vec<Key>,
  /// keys that take effect on the next reconnect whose value changed since the last one
  pub pending_reconnect: Vec<Key>,
  /// the tool that manages the files, which are then never written
  pub managed_by: Option<String>,
  pub(crate) schema: Arc<Schema>,
  /// `values` as a tree
  pub(crate) tree: Value,
  pub(crate) file_of: BTreeMap<Key, String>,
}

/// A revision no values had before.
pub(crate) fn new_revision() -> String {
  format!("{:016x}", rand::random::<u64>())
}

/// A value the files hold that is not the one in effect.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct NotApplied {
  /// why it is not in effect
  pub reason: String,
  /// the value the failed reload read, null for none
  #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
  pub value: Value,
}

impl<T> Loaded<T> {
  /// The schema of the configuration's type.
  pub fn schema(&self) -> &Schema {
    &self.schema
  }

  /// The values in effect as a tree, secrets and all.
  pub fn tree(&self) -> &Value {
    &self.tree
  }

  /// The values with every secret shown as `*****`; null and empty secrets stay as they are.
  pub fn redacted(&self) -> Value {
    self.schema.redact(&self.tree)
  }

  /// The last file that sets `key`, `None` when no file does; a runtime override on top does
  /// not change it.
  pub fn file_setting(&self, key: &str) -> Option<&str> {
    self.file_of.get(key).map(String::as_str)
  }

  /// The file a write of `key` goes to: the last file that sets it, else the first file.
  pub fn file_for(&self, key: &str) -> Option<&str> {
    self
      .file_setting(key)
      .or_else(|| self.files.first().map(|f| f.name.as_str()))
  }
}
