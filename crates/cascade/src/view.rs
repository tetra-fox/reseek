//! Everything a settings page needs about a configuration, as one serializable document.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::Key;
use crate::describe::{Apply, Kind};
use crate::loaded::{Loaded, NotApplied};
use crate::source::Source;
use crate::write::writability;

/// The configuration in effect with, for every key, where its value came from and how it can
/// be changed.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
  /// the values in effect, secrets shown as `*****`
  #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
  pub values: Value,
  /// the revision of those values, which a change made against them names
  pub revision: String,
  /// every leaf key, by key
  pub fields: BTreeMap<Key, FieldView>,
  /// the configuration files, in the order they apply
  pub files: Vec<FileView>,
  /// keys that only take effect on restart whose value changed since the start
  pub pending_restart: Vec<Key>,
  /// keys that take effect on the next reconnect whose value changed since the last one
  pub pending_reconnect: Vec<Key>,
  /// the tool that manages the files, which are then read-only
  pub managed_by: Option<String>,
  /// keys, flags and variables that were ignored, each with where it was found
  pub warnings: Vec<String>,
  /// why the last reload failed, `None` once one succeeds
  pub load_error: Option<String>,
  /// the runtime overrides in effect, by key
  pub overrides: BTreeMap<Key, OverrideView>,
}

/// One key: where its value came from and how it can be changed.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FieldView {
  /// the layer whose value is in effect
  pub source: Source,
  /// the file a write of the key goes to: the last file that sets it, else the first file
  pub file: Option<String>,
  /// the environment variable that sets the key
  pub env_var: Option<String>,
  /// the long command-line flag that sets the key
  pub cli_flag: Option<String>,
  /// what kind of value the key holds
  pub kind: Kind,
  /// the key's help text
  pub doc: String,
  /// the names the value is one of, `None` when it is not one of a fixed set
  pub choices: Option<Vec<String>>,
  /// the value when no layer sets the key, null for none; `*****` for a secret that has one
  #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
  pub default: Value,
  /// when a change to the value takes effect
  pub apply: Apply,
  /// whether the value is shown as `*****`
  pub secret: bool,
  /// the files' value of the key when it is not the one in effect, and why; a secret's value is
  /// shown as `*****`
  pub not_applied: Option<NotApplied>,
  /// the fields of each element of a list of tables or entry of a user-keyed map, so a client
  /// can edit them one by one
  pub entries: Vec<EntryView>,
}

/// One field of the elements of a list of tables or the entries of a user-keyed map.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
  /// the dotted key, relative to the element
  pub key: Key,
  /// what kind of value the key holds
  pub kind: Kind,
  /// the key's help text
  pub doc: String,
  /// the names the value is one of, `None` when it is not one of a fixed set
  pub choices: Option<Vec<String>>,
  /// what a new entry holds until something sets the field, null for nothing, a secret's or
  /// one of a list's elements
  #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
  pub default: Value,
  /// whether the value is shown as `*****`
  pub secret: bool,
}

/// One configuration file.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FileView {
  /// as the files were given, e.g. `config.d/10-shares.toml`
  pub name: String,
  /// where the file is
  pub path: String,
  /// `None` when the file does not exist
  pub revision: Option<String>,
  /// whether a write to the file can be made
  pub writable: bool,
  /// why the file cannot be written
  pub read_only_reason: Option<String>,
}

/// A runtime override in effect.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OverrideView {
  /// shown as `*****` for a secret
  #[cfg_attr(feature = "utoipa", schema(value_type = Object))]
  pub value: Value,
  /// when it ends on its own, `None` for never
  pub expires_at: Option<DateTime<Utc>>,
  /// it ends with the process
  pub until_restart: bool,
  /// who set it
  pub set_by: String,
  /// when it was set
  pub set_at: DateTime<Utc>,
}

impl ConfigView {
  /// The view of `loaded`. Each file's writability is checked now.
  pub fn new<T>(loaded: &Loaded<T>) -> Self {
    let schema = loaded.schema();
    let fields = schema
      .fields()
      .iter()
      .map(|field| {
        let view = FieldView {
          source: loaded
            .sources
            .get(&field.key)
            .cloned()
            .unwrap_or(Source::Default),
          file: loaded.file_for(&field.key).map(str::to_string),
          env_var: field.env.clone(),
          cli_flag: field.cli.clone(),
          kind: field.kind.clone(),
          doc: field.doc.clone(),
          choices: field.choices.clone(),
          default: schema.redact_value(&field.key, &field.default),
          apply: field.apply,
          secret: field.secret,
          not_applied: loaded.not_applied.get(&field.key).map(|n| NotApplied {
            reason: n.reason.clone(),
            value: schema.redact_value(&field.key, &n.value),
          }),
          entries: field
            .entries
            .iter()
            .map(|entry| EntryView {
              key: entry.key.clone(),
              kind: entry.kind.clone(),
              doc: entry.doc.clone(),
              choices: entry.choices.clone(),
              default: if entry.secret {
                Value::Null
              } else {
                entry.default.clone()
              },
              secret: entry.secret,
            })
            .collect(),
        };
        (field.key.clone(), view)
      })
      .collect();
    let files = loaded
      .files
      .iter()
      .map(|file| {
        let writable = writability(&file.path, loaded.managed_by.as_deref());
        FileView {
          name: file.name.clone(),
          path: file.path.display().to_string(),
          revision: file.revision.clone(),
          writable: writable.is_ok(),
          read_only_reason: writable.err(),
        }
      })
      .collect();
    let overrides = loaded
      .overrides
      .iter()
      .map(|(key, o)| {
        let view = OverrideView {
          value: schema.redact_value(key, &o.value),
          expires_at: o.expires_at,
          until_restart: o.until_restart,
          set_by: o.set_by.clone(),
          set_at: o.set_at,
        };
        (key.clone(), view)
      })
      .collect();
    Self {
      values: loaded.redacted(),
      revision: loaded.revision.clone(),
      fields,
      files,
      pending_restart: loaded.pending_restart.clone(),
      pending_reconnect: loaded.pending_reconnect.clone(),
      managed_by: loaded.managed_by.clone(),
      warnings: loaded.warnings.clone(),
      load_error: loaded.load_error.clone(),
      overrides,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::Lasts;
  use crate::testing::{example_layers, pairs, values, watched, write};
  use serde_json::json;

  #[tokio::test]
  async fn the_view_says_where_each_value_came_from() {
    let dir = tempfile::tempdir().unwrap();
    write(
      dir.path(),
      "config.toml",
      "[server]\naddress = \"file\"\npassword = \"hunter2\"\n",
    );
    write(dir.path(), "config.d/10-a.toml", "[server]\nport = 3000\n");
    let watcher = watched(dir.path());
    watcher
      .set_overrides(
        values(&[("server.address", json!("runtime"))]),
        vec![],
        "admin",
        Lasts::UntilRemoved,
        None,
      )
      .await
      .unwrap();
    let view = serde_json::to_value(ConfigView::new(&watcher.current())).unwrap();

    assert_eq!(view["values"]["server"]["password"], json!("*****"));
    assert_eq!(view["values"]["server"]["address"], json!("runtime"));
    assert_eq!(
      view["values"]["webhooks"],
      json!([]),
      "values keep their configuration keys"
    );
    assert_eq!(
      view["fields"]["server.password"],
      json!({
        "source": { "layer": "file", "file": "config.toml" },
        "file": "config.toml",
        "envVar": "APP_SERVER_PASSWORD",
        "cliFlag": "--password",
        "kind": "string",
        "doc": "The password to log in with.",
        "choices": null,
        "default": null,
        "apply": "reconnect",
        "secret": true,
        "notApplied": null,
        "entries": [],
      })
    );
    assert_eq!(
      view["fields"]["server.port"]["file"],
      json!("config.d/10-a.toml")
    );
    assert_eq!(
      view["fields"]["server.username"]["source"]["var"],
      json!("APP_USERNAME")
    );
    assert_eq!(
      view["fields"]["server.address"]["source"]["layer"],
      json!("runtime")
    );
    assert_eq!(
      view["fields"]["flags.volatile"]["envVar"],
      json!("APP_VOLATILE")
    );
    assert_eq!(view["fields"]["api_keys"]["cliFlag"], json!(null));
    assert_eq!(view["fields"]["api_keys"]["kind"], json!("map"));
    assert_eq!(
      view["fields"]["shares.limits"]["kind"],
      json!({ "list": "integer" })
    );
    assert_eq!(
      view["fields"]["webhooks"]["kind"],
      json!({ "list": "table" })
    );
    assert_eq!(view["fields"]["server.buffer"]["kind"], json!("size"));
    assert_eq!(
      view["fields"]["shares.cache.mode"]["choices"],
      json!(["memory", "disk"])
    );
    assert_eq!(
      view["fields"]["shares.cache.mode"]["default"],
      json!("memory")
    );
    assert_eq!(view["fields"]["server.timeout"]["choices"], json!(null));
    assert_eq!(view["fields"]["server.timeout"]["default"], json!("5s"));
    assert_eq!(
      view["overrides"]["server.address"]["value"],
      json!("runtime")
    );
    assert_eq!(view["overrides"]["server.address"]["setBy"], json!("admin"));
    assert_eq!(
      view["overrides"]["server.address"]["expiresAt"],
      json!(null)
    );
    assert_eq!(view["files"][0]["name"], json!("config.toml"));
    assert_eq!(view["files"][0]["writable"], json!(true));
    assert_eq!(view["files"][0]["readOnlyReason"], json!(null));
    assert_eq!(view["files"][1]["name"], json!("config.d/10-a.toml"));
    assert!(
      view["files"][1]["revision"]
        .as_str()
        .unwrap()
        .starts_with("sha256:")
    );
    for key in [
      "pendingRestart",
      "pendingReconnect",
      "managedBy",
      "warnings",
      "loadError",
    ] {
      assert!(view.get(key).is_some(), "{key} is written even when empty");
    }
  }

  #[test]
  fn a_managed_configuration_is_read_only_and_secret_overrides_are_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let mut layers = example_layers(dir.path())
      .managed_by(Some("nixos".into()))
      .env(pairs(&[("APP_DEBUG", "true")]));
    let mut loaded = layers.load().unwrap();
    loaded.overrides.insert(
      "server.password".into(),
      crate::Override {
        value: json!("secret"),
        declared: json!(null),
        set_by: "admin".into(),
        set_at: chrono::Utc::now(),
        expires_at: None,
        until_restart: false,
      },
    );
    loaded.not_applied.insert(
      "server.password".into(),
      NotApplied {
        reason: "refused".into(),
        value: json!("hunter2"),
      },
    );
    let view = ConfigView::new(&loaded);
    assert_eq!(
      view.fields["server.password"].not_applied,
      Some(NotApplied {
        reason: "refused".into(),
        value: json!("*****"),
      })
    );
    assert_eq!(view.managed_by.as_deref(), Some("nixos"));
    assert!(!view.files[0].writable);
    assert_eq!(
      view.files[0].read_only_reason.as_deref(),
      Some("managed by nixos")
    );
    assert_eq!(view.overrides["server.password"].value, json!("*****"));
    assert_eq!(
      view.fields["debug"].source,
      Source::Env {
        var: "APP_DEBUG".into()
      }
    );
  }
}
