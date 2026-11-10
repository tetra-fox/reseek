//! Changes made through the watcher: runtime overrides and writes to the files.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use serde_json::Value;
use tracing::{debug, info};

use super::{Change, FileEdit, Read, Shared, State, Written};
use crate::describe::Apply;
use crate::edit::FileEditor;
use crate::files::{read_optional_text, revision_of};
use crate::overrides::{Lasts, Override};
use crate::schema::Schema;
use crate::source::Source;
use crate::write::writability;
use crate::{Error, Key, Root, tree};

impl<T: Root> State<T> {
  /// Sets and removes runtime overrides, saving them before they take effect. For a secret,
  /// `*****` (what a secret is shown as) leaves its override as it is.
  pub(super) fn change_overrides(
    &mut self,
    shared: &Shared<T>,
    set: BTreeMap<Key, Value>,
    unset: &[Key],
    set_by: &str,
    lasts: Lasts,
  ) -> Result<Option<Arc<Change<T>>>, Error> {
    let declared = self.declared();
    let now = Utc::now();
    let schema = self.layers.schema_arc().clone();
    let (mut items, dropped) =
      self
        .layers
        .overrides
        .pruned(&schema, &declared.tree, &declared.sources, now);
    for key in unset {
      items.remove(key);
    }
    for (key, value) in set {
      let field = schema
        .field(&key)
        .ok_or_else(|| Error::UnknownKey(key.clone()))?;
      if field.keeps(&value) {
        continue;
      }
      if field.apply == Apply::Restart {
        return Err(Error::NeedsRestart(key));
      }
      if let Some(by) = declared.sources.get(&key).and_then(Source::fixed_by) {
        return Err(Error::SetBy {
          key,
          by: by.to_string(),
        });
      }
      if value.is_null() {
        return Err(Error::Value {
          key,
          by: "a runtime override".into(),
          message: "an override needs a value; remove it to go back to the declared one".into(),
        });
      }
      // null when nothing below sets the key
      let declared_value = tree::get(&declared.tree, &key).cloned().unwrap_or_default();
      let item = Override {
        value,
        declared: declared_value,
        set_by: set_by.to_string(),
        set_at: now,
        expires_at: match lasts {
          Lasts::Until(at) => Some(at),
          Lasts::UntilRemoved | Lasts::UntilRestart => None,
        },
        until_restart: lasts == Lasts::UntilRestart,
      };
      items.insert(key, item);
    }
    let loaded = self
      .layers
      .finish(&declared, &items)
      .map_err(|failure| failure.error)?;
    self.layers.overrides.replace(items)?;
    for (key, reason) in dropped {
      info!(key, reason, "runtime override dropped");
    }
    for key in loaded.overrides.keys() {
      debug!(key, set_by, "runtime override in effect");
    }
    let change = self.publish(shared, loaded);
    // after the publish, so the task reads the new overrides when it wakes
    shared.overrides_changed.notify_one();
    Ok(change)
  }

  /// Changes the file `name` if it is still at `revision`, after the checks
  /// [`super::Watcher::patch_file`] lists, then loads the files again.
  pub(super) fn write_file(
    &mut self,
    shared: &Shared<T>,
    name: &str,
    revision: Option<&str>,
    edit: FileEdit,
  ) -> Result<Written<T>, Error> {
    let path = self
      .layers
      .file_path(name)?
      .ok_or_else(|| Error::UnknownFile(name.to_string()))?;
    let current = read_optional_text(&path)?;
    let base = current.as_deref().map(|t| revision_of(t.as_bytes()));
    if base.as_deref() != revision {
      return Err(Error::Stale);
    }
    writability(&path, self.layers.managed()).map_err(|reason| Error::ReadOnly {
      file: name.to_string(),
      reason,
    })?;
    let schema = self.layers.schema_arc().clone();
    let editor = FileEditor::new(&schema);
    let old = current.unwrap_or_default();
    let text = match edit {
      FileEdit::Patch { set, unset } => editor.patch(name, &old, &set, &unset)?,
      FileEdit::Replace(text) => text,
    };
    // the files as they are now with the edit, not as last loaded: a file that failed to load
    // still has its place in the order
    let declared = self.layers.declare(Some((name, &text)))?;
    if let Some(error) = self.layers.refusal(&declared) {
      return Err(error);
    }
    let changed = changed_in_file(&schema, &old, &text);
    // the sources before the edit: after it, the file would win over an env var
    let before = self.declared();
    for key in &changed {
      if let Some(by) = before.sources.get(key).and_then(Source::fixed_by) {
        return Err(Error::SetBy {
          key: key.clone(),
          by: by.to_string(),
        });
      }
    }
    let position = |file: &str| declared.files.iter().position(|f| f.name == file);
    for key in changed {
      // a file later in the order wins, so the write would not take effect
      if let Some(later) = declared.file_of.get(&key)
        && position(later) > position(name)
      {
        return Err(Error::SetBy {
          key,
          by: later.clone(),
        });
      }
    }
    let revision = editor.write(&path, base.as_deref(), &text)?;
    info!(file = name, path = %path.display(), "configuration file written");
    match self.load(shared) {
      Ok(change) => Ok(Written { revision, change }),
      Err(failure) => Err(self.report(shared, failure, Read::Files)),
    }
  }
}

/// The keys whose value differs between two texts of one file. Text that does not parse sets
/// nothing.
fn changed_in_file(schema: &Schema, old: &str, new: &str) -> Vec<Key> {
  let tree_of = |text: &str| {
    text
      .parse::<toml::Table>()
      .ok()
      .and_then(|t| tree::from_toml(toml::Value::Table(t)).ok())
      .unwrap_or_default()
  };
  schema.diff_trees(&tree_of(old), &tree_of(new))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::overrides::Overrides;
  use crate::schema::REDACTED;
  use crate::testing::{next_change, values, watched, watched_layers};
  use crate::{Watch, Watcher};
  use serde_json::json;
  use std::fs;
  use tokio_util::sync::CancellationToken;

  #[tokio::test]
  async fn overrides_apply_expire_and_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let watcher = watched(dir.path());
    let mut changes = watcher.changes();
    let cancel = CancellationToken::new();
    watcher.start(Watch::Off, cancel.clone());

    let soon = Utc::now() + chrono::Duration::milliseconds(300);
    let change = watcher
      .set_overrides(
        values(&[("server.port", json!(5000))]),
        vec![],
        "admin",
        Lasts::Until(soon),
        None,
      )
      .await
      .unwrap()
      .unwrap();
    assert_eq!(change.by_apply.reconnect, ["server.port"]);
    let current = watcher.current();
    assert_eq!(current.values.server.port, 5000);
    assert!(
      matches!(&current.sources["server.port"], Source::Runtime { set_by, .. } if set_by == "admin")
    );
    assert_eq!(current.pending_reconnect, ["server.port"]);
    assert!(next_change(&mut changes).await.changed == ["server.port"]);

    let expired = next_change(&mut changes).await;
    assert_eq!(
      expired.current.values.server.port, 2242,
      "back when it expires"
    );
    assert_eq!(expired.current.sources["server.port"], Source::Default);
    assert!(
      Overrides::load(dir.path().join("overrides.toml"))
        .unwrap()
        .items()
        .is_empty()
    );

    let refused = |e: Result<_, Error>| e.unwrap_err();
    let e = refused(
      watcher
        .set_overrides(
          values(&[("debug", json!(true))]),
          vec![],
          "a",
          Lasts::UntilRemoved,
          None,
        )
        .await,
    );
    assert!(matches!(e, Error::NeedsRestart(k) if k == "debug"));
    let e = refused(
      watcher
        .set_overrides(
          values(&[("server.username", json!("x"))]),
          vec![],
          "a",
          Lasts::UntilRemoved,
          None,
        )
        .await,
    );
    assert!(matches!(e, Error::SetBy { by, .. } if by == "APP_USERNAME"));
    let e = refused(
      watcher
        .set_overrides(
          values(&[("nope", json!(1))]),
          vec![],
          "a",
          Lasts::UntilRemoved,
          None,
        )
        .await,
    );
    assert!(matches!(e, Error::UnknownKey(_)));
    let e = refused(
      watcher
        .set_overrides(
          values(&[("server.port", json!("x"))]),
          vec![],
          "a",
          Lasts::UntilRemoved,
          None,
        )
        .await,
    );
    assert!(
      matches!(&e, Error::Value { by, .. } if by == "a runtime override"),
      "{e}"
    );
    let e = refused(
      watcher
        .set_overrides(
          values(&[("server.port", json!(80))]),
          vec![],
          "a",
          Lasts::UntilRemoved,
          None,
        )
        .await,
    );
    assert!(matches!(e, Error::Invalid(_)));
    assert!(
      watcher.current().overrides.is_empty(),
      "nothing refused is in effect"
    );
    assert!(matches!(
      watcher.remove_override("server.port", None).await,
      Err(Error::NoOverride(_))
    ));

    watcher
      .set_overrides(
        values(&[("server.address", json!("o"))]),
        vec![],
        "a",
        Lasts::UntilRemoved,
        None,
      )
      .await
      .unwrap();
    let change = watcher
      .remove_override("server.address", None)
      .await
      .unwrap()
      .unwrap();
    assert_eq!(change.current.values.server.address, "localhost");

    watcher
      .set_overrides(
        values(&[("server.password", json!("pw"))]),
        vec![],
        "a",
        Lasts::UntilRemoved,
        None,
      )
      .await
      .unwrap();
    let kept = watcher
      .set_overrides(
        values(&[("server.password", json!(REDACTED))]),
        vec![],
        "a",
        Lasts::UntilRemoved,
        None,
      )
      .await
      .unwrap();
    assert!(
      kept.is_none(),
      "`*****` leaves a secret's override as it is"
    );
    assert_eq!(
      watcher.current().values.server.password.as_deref(),
      Some("pw")
    );
    cancel.cancel();
  }

  #[tokio::test]
  async fn a_map_entry_keeps_the_secret_it_shows_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
      &path,
      "[api_keys.robot]\nkey = \"real\"\nrole = \"reader\"\n",
    )
    .unwrap();
    let watcher = watched(dir.path());
    let revision = watcher.current().files[0].revision.clone();
    // a client changes one field of the entry and sends back the whole map as it read it
    let mut shown = watcher.current().redacted()["api_keys"].clone();
    assert_eq!(shown["robot"]["key"], json!(REDACTED));
    shown["robot"]["role"] = json!("writer");

    watcher
      .patch_file(
        "config.toml",
        revision.as_deref(),
        None,
        values(&[("api_keys", shown.clone())]),
        vec![],
      )
      .await
      .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("real") && !text.contains(REDACTED), "{text}");
    let robot = watcher.current().values.api_keys["robot"].clone();
    assert_eq!(
      (robot.key.as_str(), robot.role.as_str()),
      ("real", "writer")
    );

    watcher
      .set_overrides(
        values(&[("api_keys", shown)]),
        vec![],
        "a",
        Lasts::UntilRemoved,
        None,
      )
      .await
      .unwrap();
    assert_eq!(watcher.current().values.api_keys["robot"].key, "real");
  }

  #[tokio::test]
  async fn a_change_made_against_values_since_replaced_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let watcher = watched(dir.path());
    let read = watcher.current().revision.clone();
    let port = |port: u16| values(&[("server.port", json!(port))]);
    watcher
      .set_overrides(port(5000), vec![], "a", Lasts::UntilRemoved, Some(&read))
      .await
      .unwrap();
    let now = watcher.current().revision.clone();
    assert_ne!(now, read, "a change gives the values a new revision");

    // a second admin read the values before the first one's change
    let outdated = watcher
      .set_overrides(port(6000), vec![], "b", Lasts::UntilRemoved, Some(&read))
      .await;
    assert!(matches!(&outdated, Err(Error::Outdated(current)) if *current == now));
    let written = watcher
      .patch_file("config.toml", None, Some(&read), port(6000), vec![])
      .await;
    assert!(matches!(written, Err(Error::Outdated(_))));
    assert_eq!(watcher.current().values.server.port, 5000);

    watcher.reload().await.unwrap();
    assert_eq!(
      watcher.current().revision,
      now,
      "a reload that changes nothing keeps it"
    );
    watcher
      .patch_file("config.toml", None, Some(&now), port(6000), vec![])
      .await
      .unwrap();
  }

  #[tokio::test]
  async fn a_hidden_secret_with_nothing_to_keep_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[api_keys.robot]\nkey = \"real\"\n").unwrap();
    let before = fs::read_to_string(&path).unwrap();
    let watcher = watched(dir.path());
    let refused = async |set: BTreeMap<Key, Value>| {
      let revision = watcher.current().files[0].revision.clone();
      let written = watcher
        .patch_file("config.toml", revision.as_deref(), None, set, vec![])
        .await;
      match written {
        Err(Error::Invalid(invalid)) => invalid[0].to_string(),
        other => panic!("{:?}", other.map(|_| ())),
      }
    };
    // an entry under a new name, whose secret nothing can tell
    let renamed = json!({ "other": { "key": REDACTED, "role": "r" } });
    let message = refused(values(&[("api_keys", renamed)])).await;
    assert!(message.starts_with("api_keys: "), "{message}");
    assert!(message.contains("api_keys.other.key"), "{message}");
    // an element of a list, which can move
    let list = json!([{ "url": "http://x", "token": REDACTED }]);
    let message = refused(values(&[("webhooks", list)])).await;
    assert!(message.contains("webhooks[0].token"), "{message}");
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
  }

  #[tokio::test]
  async fn patching_a_file_applies_it_and_reports_what_changed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "# mine\n\n[server]\nport = 2000 # here\n").unwrap();
    let watcher = watched(dir.path());
    let revision = watcher.current().files[0].revision.clone();

    let written = watcher
      .patch_file(
        "config.toml",
        revision.as_deref(),
        None,
        values(&[
          ("server.port", json!(3000)),
          ("debug", json!(true)),
          ("server.password", json!("pw")),
        ]),
        vec![],
      )
      .await
      .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
      text.starts_with("# mine\n") && text.contains("port = 3000 # here"),
      "{text}"
    );
    assert_eq!(written.revision, revision_of(text.as_bytes()));
    assert_eq!(
      fs::read_to_string(dir.path().join("config.toml.bak")).unwrap(),
      "# mine\n\n[server]\nport = 2000 # here\n"
    );
    let change = written.change.unwrap();
    assert_eq!(change.by_apply.restart, ["debug"]);
    assert_eq!(
      change.by_apply.reconnect,
      ["server.port", "server.password"]
    );
    let current = watcher.current();
    assert_eq!(current.pending_restart, ["debug"]);
    assert_eq!(
      current.pending_reconnect,
      ["server.port", "server.password"]
    );
    assert_eq!(
      current.files[0].revision.as_deref(),
      Some(written.revision.as_str())
    );
    watcher.reconnected().await.unwrap();
    assert!(watcher.current().pending_reconnect.is_empty());
    assert_eq!(watcher.current().pending_restart, ["debug"]);

    let same = watcher
      .patch_file(
        "config.toml",
        Some(&written.revision),
        None,
        values(&[("server.password", json!(REDACTED))]),
        vec![],
      )
      .await
      .unwrap();
    assert!(same.change.is_none(), "`*****` keeps the secret");
    assert_eq!(
      watcher.current().values.server.password.as_deref(),
      Some("pw")
    );

    let stale = watcher
      .patch_file(
        "config.toml",
        revision.as_deref(),
        None,
        values(&[("server.port", json!(4000))]),
        vec![],
      )
      .await;
    assert!(matches!(stale, Err(Error::Stale)));

    let now = watcher.current().files[0].revision.clone();
    let invalid = watcher
      .patch_file(
        "config.toml",
        now.as_deref(),
        None,
        values(&[("server.port", json!(80))]),
        vec![],
      )
      .await;
    assert!(matches!(invalid, Err(Error::Invalid(_))));
    let fixed = watcher
      .patch_file(
        "config.toml",
        now.as_deref(),
        None,
        values(&[("server.username", json!("x"))]),
        vec![],
      )
      .await;
    assert!(matches!(fixed, Err(Error::SetBy { by, .. }) if by == "APP_USERNAME"));
    assert_eq!(
      crate::files::revision(&path).unwrap(),
      now,
      "nothing refused reaches the file"
    );

    let unknown = watcher
      .patch_file("other.toml", None, None, BTreeMap::new(), vec![])
      .await;
    assert!(matches!(unknown, Err(Error::UnknownFile(_))));
  }

  #[tokio::test]
  async fn a_key_a_later_file_sets_is_written_there() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("config.d")).unwrap();
    fs::write(
      dir.path().join("config.d/10-a.toml"),
      "[server]\nport = 2000\n",
    )
    .unwrap();
    let watcher = watched(dir.path());
    let e = watcher
      .patch_file(
        "config.toml",
        None,
        None,
        values(&[("server.port", json!(3000))]),
        vec![],
      )
      .await
      .unwrap_err();
    assert!(
      matches!(&e, Error::SetBy { key, by } if key == "server.port" && by == "config.d/10-a.toml"),
      "{e}"
    );
    assert!(!dir.path().join("config.toml").exists());

    let current = watcher.current();
    let file = current.file_for("server.port").unwrap();
    let revision = current
      .files
      .iter()
      .find(|f| f.name == file)
      .unwrap()
      .revision
      .clone();
    watcher
      .patch_file(
        file,
        revision.as_deref(),
        None,
        values(&[("server.port", json!(3000))]),
        vec![],
      )
      .await
      .unwrap();
    assert_eq!(watcher.current().values.server.port, 3000);

    // a drop-in that arrives broken is not loaded, and can still be fixed through its own file
    fs::write(dir.path().join("config.d/20-b.toml"), "[server]\nport = \n").unwrap();
    assert!(watcher.reload().await.is_err());
    let broken = revision_of(b"[server]\nport = \n");
    watcher
      .replace_file(
        "config.d/20-b.toml",
        Some(&broken),
        "[server]\nport = 4000\n".into(),
      )
      .await
      .unwrap();
    assert_eq!(watcher.current().values.server.port, 4000);
    let revision = watcher
      .current()
      .files
      .iter()
      .find(|f| f.name == "config.d/10-a.toml")
      .unwrap()
      .revision
      .clone();
    let e = watcher
      .patch_file(
        "config.d/10-a.toml",
        revision.as_deref(),
        None,
        values(&[("server.port", json!(5000))]),
        vec![],
      )
      .await
      .unwrap_err();
    assert!(
      matches!(&e, Error::SetBy { by, .. } if by == "config.d/20-b.toml"),
      "{e}"
    );
  }

  #[tokio::test]
  async fn replacing_a_file_has_the_same_checks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let watcher = watched(dir.path());

    let written = watcher
      .replace_file("config.toml", None, "[server]\nport = 3000\n".into())
      .await
      .unwrap();
    assert_eq!(written.change.unwrap().changed, ["server.port"]);
    let e = watcher
      .replace_file(
        "config.toml",
        Some(&written.revision),
        "[server]\nusername = \"x\"\n".into(),
      )
      .await
      .unwrap_err();
    assert!(matches!(e, Error::SetBy { .. }), "{e}");
    assert_eq!(
      watcher
        .check_file("config.toml", "[server]\nprot = 1\n")
        .await
        .unwrap()
        .warnings,
      ["config.toml: unknown key `server.prot`"]
    );
    assert_eq!(
      fs::read_to_string(&path).unwrap(),
      "[server]\nport = 3000\n"
    );
  }

  #[tokio::test]
  async fn a_managed_configuration_is_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let watcher =
      Watcher::new(watched_layers(dir.path()).managed_by(Some("nixos".into()))).unwrap();
    let e = watcher
      .patch_file(
        "config.toml",
        None,
        None,
        values(&[("server.port", json!(3000))]),
        vec![],
      )
      .await
      .unwrap_err();
    assert!(
      matches!(&e, Error::ReadOnly { reason, .. } if reason == "managed by nixos"),
      "{e}"
    );
    assert!(!dir.path().join("config.toml").exists());
    watcher
      .set_overrides(
        values(&[("server.port", json!(3000))]),
        vec![],
        "admin",
        Lasts::UntilRemoved,
        None,
      )
      .await
      .expect("overrides still work, they are kept elsewhere");
  }
}
