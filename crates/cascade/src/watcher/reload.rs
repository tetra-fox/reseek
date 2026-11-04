//! Loading the files again and announcing what changed, or keeping the values in effect and
//! saying why when the load fails.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use serde_json::Value;
use tracing::{debug, error, info};

use super::{Attempt, ByApply, Change, Failed, Shared, State};
use crate::describe::Apply;
use crate::layers::{Declared, Failure, error_keys};
use crate::loaded::{Loaded, NotApplied};
use crate::schema::Schema;
use crate::{Error, Key, Root, tree};

impl<T: Root> State<T> {
  /// Loads the files, unless `only_if_changed` and they are as the last attempt read them.
  pub(super) fn attempt(&mut self, shared: &Shared<T>, only_if_changed: bool) -> Attempt<T> {
    // a file that cannot be read counts as changed, and loading it says why
    if let Ok(files) = self.layers.files_now()
      && only_if_changed
      && files == self.attempted
    {
      debug!("configuration files are as last read");
      return Attempt::Unchanged;
    }
    match self.load(shared) {
      Ok(change) => Attempt::Done(change),
      Err(failure) => Attempt::Failed(failure),
    }
  }

  /// Loads the files and makes them the configuration in effect.
  pub(super) fn load(&mut self, shared: &Shared<T>) -> Result<Option<Arc<Change<T>>>, Failure> {
    // a file that cannot be read fails the load below, which says why
    if let Ok(files) = self.layers.files_now() {
      self.attempted = files;
    }
    let loaded = self.layers.load_at(Utc::now())?;
    self.failed = None;
    Ok(self.publish(shared, loaded))
  }

  /// Puts the runtime layer on the last files that loaded again, without reading them, so an
  /// override still ends on time while a file is broken.
  pub(super) fn reapply(&mut self, shared: &Shared<T>) -> Result<Option<Arc<Change<T>>>, Error> {
    let declared = self.declared();
    match self.layers.apply(&declared, Utc::now()) {
      Ok(loaded) => Ok(self.publish(shared, loaded)),
      Err(failure) => Err(self.report(shared, failure, Read::Overrides)),
    }
  }

  /// The layers below the runtime one, as the last load that succeeded merged them.
  pub(super) fn declared(&self) -> Declared {
    self
      .layers
      .last
      .clone()
      .expect("the first load, which the watcher is made with, keeps its layers")
  }

  /// Makes `loaded` the configuration in effect, and announces what changed.
  pub(super) fn publish(
    &mut self,
    shared: &Shared<T>,
    mut loaded: Loaded<T>,
  ) -> Option<Arc<Change<T>>> {
    if let Some(failed) = &self.failed {
      failed.show(&mut loaded);
    }
    let previous = shared.current.borrow().clone();
    let schema = loaded.schema.clone();
    loaded.pending_restart = pending(&schema, &self.startup, &loaded.tree, Apply::Restart);
    loaded.pending_reconnect = pending(&schema, &self.reconnected, &loaded.tree, Apply::Reconnect);
    let mut changed = Vec::new();
    let mut by_apply = ByApply::default();
    for field in schema.changed_fields(&previous.tree, &loaded.tree) {
      let key = &field.key;
      let old = tree::get(&previous.tree, key).unwrap_or(&Value::Null);
      let new = tree::get(&loaded.tree, key).unwrap_or(&Value::Null);
      debug!(
        key,
        apply = ?field.apply,
        old = %schema.redact_value(key, old),
        new = %schema.redact_value(key, new),
        "configuration value changed"
      );
      changed.push(key.clone());
      match field.apply {
        Apply::Live => by_apply.live.push(key.clone()),
        Apply::Reconnect => by_apply.reconnect.push(key.clone()),
        Apply::Restart => by_apply.restart.push(key.clone()),
      }
    }
    if changed.is_empty() {
      debug!("configuration unchanged");
      // nothing a reader sees is different, so readers are not woken
      let same = previous.load_error == loaded.load_error
        && previous.load_error_at == loaded.load_error_at
        && previous.not_applied == loaded.not_applied
        && previous.sources == loaded.sources
        && previous.file_of == loaded.file_of
        && previous.warnings == loaded.warnings
        && previous.files == loaded.files
        && previous.overrides == loaded.overrides
        && previous.pending_reconnect == loaded.pending_reconnect;
      if !same {
        shared.current.send_replace(Arc::new(loaded));
      }
      return None;
    }
    let current = Arc::new(loaded);
    shared.current.send_replace(current.clone());
    info!(
      changed = changed.len(),
      reconnect = by_apply.reconnect.len(),
      restart = by_apply.restart.len(),
      "configuration changed"
    );
    let change = Arc::new(Change {
      changed,
      by_apply,
      previous,
      current,
    });
    // an error only means nobody is listening
    drop(shared.changes.send(change.clone()));
    Some(change)
  }

  /// Keeps the values in effect after a failed load, and records why it failed and which of the
  /// files' values are held back. A failure to read the files stays shown until they load; one
  /// of the overrides on the last files that loaded lasts until the next change.
  pub(super) fn report(&mut self, shared: &Shared<T>, failure: Failure, read: Read) -> Error {
    let message = failure.error.to_string();
    let files: Vec<String> = self
      .attempted
      .iter()
      .map(|f| f.path.display().to_string())
      .collect();
    error!(
      error = %message,
      files = files.join(", "),
      "configuration not loaded, the values in effect stay"
    );
    let previous = shared.current.borrow().clone();
    let not_applied = match &failure.tree {
      Some(tree) => {
        let keys = error_keys(&failure.error);
        previous
          .schema
          .diff_trees(&previous.tree, tree)
          .into_iter()
          .map(|key| {
            let reason = if keys.contains(key.as_str()) {
              reason_for(&failure.error, &key)
            } else {
              message.clone()
            };
            let value = tree::get(tree, &key).cloned().unwrap_or(Value::Null);
            (key, NotApplied { reason, value })
          })
          .collect()
      }
      None => BTreeMap::new(),
    };
    let failed = Failed {
      error: message,
      at: Utc::now(),
      not_applied,
      files: self.attempted.clone(),
    };
    let mut loaded = (*previous).clone();
    failed.show(&mut loaded);
    if read == Read::Files {
      self.failed = Some(failed);
    }
    // the overrides the failed load dropped, an expired one included, are gone from the store;
    // the values they set stay in effect with the rest
    loaded.overrides = self.layers.overrides.items().clone();
    shared.current.send_replace(Arc::new(loaded));
    failure.error
  }
}

/// What a failed load read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Read {
  /// the files
  Files,
  /// only the runtime overrides, on the files as they last loaded
  Overrides,
}

/// What an error says about one of its keys.
fn reason_for(error: &Error, key: &str) -> String {
  match error {
    Error::Invalid(invalid) => invalid
      .iter()
      .filter(|i| i.key == key)
      .map(|i| i.message.as_str())
      .collect::<Vec<_>>()
      .join("; "),
    other => other.to_string(),
  }
}

/// The keys of class `apply` whose value in `tree` differs from `base`.
fn pending(schema: &Schema, base: &Value, tree: &Value, apply: Apply) -> Vec<Key> {
  schema
    .fields()
    .iter()
    .filter(|f| f.apply == apply && tree::get(base, &f.key) != tree::get(tree, &f.key))
    .map(|f| f.key.clone())
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::files::revision_of;
  use crate::overrides::{Lasts, Overrides};
  use crate::testing::{Example, example_layers, values, watched};
  use crate::{Invalid, Watch, Watcher};
  use serde_json::json;
  use std::fs;
  use std::time::Duration;
  use tokio_util::sync::CancellationToken;

  #[tokio::test]
  async fn a_bad_reload_keeps_the_values_in_effect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[server]\nport = 2000\n").unwrap();
    let watcher = watched(dir.path());

    fs::write(&path, "[server]\nport = 80\naddress = \"new\"\n").unwrap();
    let e = watcher.reload().await.unwrap_err();
    assert!(matches!(e, Error::Invalid(_)), "{e}");
    let current = watcher.current();
    assert_eq!(current.values.server.port, 2000);
    assert_eq!(current.values.server.address, "localhost");
    assert_eq!(
      current.not_applied["server.port"],
      NotApplied {
        reason: "must be 1024 or more".into(),
        value: json!(80),
      }
    );
    assert_eq!(
      current.not_applied["server.address"].reason,
      "invalid configuration: server.port: must be 1024 or more",
      "a key the failure is not about is held back with the whole reason"
    );
    assert!(current.load_error.as_deref().unwrap().contains("1024"));
    assert_eq!(
      current.files[0].revision,
      Some(revision_of(b"[server]\nport = 80\naddress = \"new\"\n")),
      "the revision on disk, so a fix can be written against it"
    );

    fs::write(&path, "[server\nport = ").unwrap();
    assert!(matches!(watcher.reload().await, Err(Error::Parse { .. })));
    let current = watcher.current();
    assert_eq!(current.values.server.port, 2000);
    assert!(current.not_applied.is_empty());
    assert!(
      current
        .load_error
        .as_deref()
        .unwrap()
        .starts_with("config.toml: line")
    );

    fs::write(&path, "[server]\nport = 3000\n").unwrap();
    let change = watcher.reload().await.unwrap().unwrap();
    assert_eq!(change.changed, ["server.port"]);
    let current = watcher.current();
    assert_eq!(current.values.server.port, 3000);
    assert_eq!(current.load_error, None);
    assert!(current.not_applied.is_empty());
  }

  #[tokio::test]
  async fn a_bad_reload_stays_reported_across_an_override_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[server]\nport = 2000\n").unwrap();
    let layers = example_layers(dir.path())
      .overrides(Overrides::load(dir.path().join("overrides.toml")).unwrap());
    let watcher = Watcher::new(layers).unwrap();
    fs::write(&path, "[server\nport = ").unwrap();
    assert!(watcher.reload().await.is_err());
    let failed = watcher.current();

    watcher
      .set_overrides(
        values(&[("server.port", json!(5000))]),
        vec![],
        "a",
        Lasts::UntilRemoved,
      )
      .await
      .unwrap();
    let current = watcher.current();
    assert_eq!(current.values.server.port, 5000);
    assert_eq!(current.load_error, failed.load_error);
    assert_eq!(current.load_error_at, failed.load_error_at);
    assert_eq!(
      current.files[0].revision,
      Some(revision_of(b"[server\nport = ")),
      "the revision on disk, so a fix can be written against it"
    );

    fs::write(&path, "[server]\nport = 3000\n").unwrap();
    watcher.reload().await.unwrap();
    assert_eq!(watcher.current().load_error, None);
  }

  #[tokio::test]
  async fn drop_ins_that_can_not_be_listed_are_not_taken_for_gone() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("config.d")).unwrap();
    fs::write(
      dir.path().join("config.d/10-a.toml"),
      "[server]\nport = 2000\n",
    )
    .unwrap();
    let watcher = watched(dir.path());
    fs::remove_dir_all(dir.path().join("config.d")).unwrap();
    fs::write(dir.path().join("config.d"), "").unwrap();
    assert!(matches!(watcher.reload().await, Err(Error::Read { .. })));
    assert_eq!(watcher.current().values.server.port, 2000);
    assert!(watcher.current().load_error.is_some());
  }

  #[tokio::test]
  async fn an_expiry_that_leaves_an_invalid_configuration_keeps_the_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[server]\nport = 10\n[shares.cache]\nworkers = 4\n").unwrap();
    let layers = example_layers(dir.path())
      .overrides(Overrides::load(dir.path().join("overrides.toml")).unwrap())
      .validate(|o: &Example| {
        if u32::from(o.server.port) < o.shares.cache.workers {
          Err(vec![Invalid::new("server.port", "fewer than the workers")])
        } else {
          Ok(())
        }
      });
    let watcher = Watcher::new(layers).unwrap();
    let cancel = CancellationToken::new();
    watcher.start(Watch::Off, cancel.clone());
    let soon = Utc::now() + chrono::Duration::milliseconds(300);
    watcher
      .set_overrides(
        values(&[("server.port", json!(100))]),
        vec![],
        "a",
        Lasts::Until(soon),
      )
      .await
      .unwrap();
    // valid only while the override lasts
    fs::write(&path, "[server]\nport = 10\n[shares.cache]\nworkers = 50\n").unwrap();
    watcher.reload().await.unwrap();
    assert_eq!(watcher.current().values.shares.cache.workers, 50);

    let mut states = watcher.subscribe();
    states.mark_unchanged();
    tokio::time::timeout(Duration::from_secs(5), states.changed())
      .await
      .unwrap()
      .unwrap();
    let current = watcher.current();
    assert_eq!(current.values.server.port, 100, "the values in effect stay");
    assert!(
      current.overrides.is_empty(),
      "the expired override is not listed"
    );
    assert!(
      current
        .load_error
        .as_deref()
        .unwrap()
        .contains("fewer than the workers")
    );
    assert_eq!(current.not_applied["server.port"].value, json!(10));
    assert!(
      Overrides::load(dir.path().join("overrides.toml"))
        .unwrap()
        .items()
        .is_empty(),
      "and it is gone from the store"
    );
    let again = tokio::time::timeout(Duration::from_millis(500), states.changed()).await;
    assert!(
      again.is_err(),
      "the failure is reported once, not again and again"
    );
    cancel.cancel();
  }

  #[tokio::test]
  async fn a_reload_that_changes_nothing_wakes_no_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[server]\nport = 2000\n").unwrap();
    let watcher = watched(dir.path());
    let mut states = watcher.subscribe();
    states.mark_unchanged();
    assert!(watcher.reload().await.unwrap().is_none());
    assert!(!states.has_changed().unwrap());

    fs::write(&path, "# a comment\n[server]\nport = 2000\n").unwrap();
    assert!(watcher.reload().await.unwrap().is_none());
    assert!(
      states.has_changed().unwrap(),
      "a new revision is news, a write needs it"
    );
  }

  #[tokio::test]
  async fn a_reload_is_tried_again_after_a_moment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let watcher = watched(dir.path());
    // an editor that truncates, then writes
    fs::write(&path, "[server]\nport = ").unwrap();
    let fixer = {
      let path = path.clone();
      tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        fs::write(&path, "[server]\nport = 4000\n").unwrap();
      })
    };
    let change = watcher.reload().await.unwrap().unwrap();
    fixer.await.unwrap();
    assert_eq!(change.changed, ["server.port"]);
    assert_eq!(watcher.current().load_error, None);
  }
}
