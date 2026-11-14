//! Runtime overrides: values set through the application while it runs, on top of every other
//! layer, kept in a TOML file so they last across restarts.
//!
//! Each override remembers the value the layers below it had when it was set. When that value
//! changes, because a file was edited or the defaults moved, the override is dropped: a new
//! declaration wins over an older runtime change. A user-keyed map's override is not dropped
//! whole: the entries it adds, changes or deletes stay over the files, and a key of an entry
//! gives way on its own when the files change it too. An override also ends when it expires, and
//! one set until restart is never written to the file, so it ends with the process.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::warn;

use crate::describe::Kind;
use crate::schema::{Field, Schema};
use crate::source::Source;
use crate::write::{self, Mode};
use crate::{Error, Key, files, tree};

/// One runtime override.
#[derive(Clone, Debug, PartialEq)]
pub struct Override {
  /// the value in effect while the override lasts
  pub value: Value,
  /// the value the layers below had when the override was set, null for none
  pub declared: Value,
  /// who set it, as the application names them
  pub set_by: String,
  /// when it was set
  pub set_at: DateTime<Utc>,
  /// when the override ends on its own, `None` for never
  pub expires_at: Option<DateTime<Utc>>,
  /// kept in memory only, never written to the file, so it ends with the process
  pub until_restart: bool,
}

/// How long a runtime override lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lasts {
  /// until it is removed, across restarts
  UntilRemoved,
  /// until this time, across restarts
  Until(DateTime<Utc>),
  /// until the process ends; it is never written to the file
  UntilRestart,
}

/// The runtime overrides, and the file they are kept in.
#[derive(Clone, Debug, Default)]
pub struct Overrides {
  /// `None` keeps them in memory only, so they end with the process
  path: Option<PathBuf>,
  items: BTreeMap<Key, Override>,
}

/// An override as the file stores it. TOML has no null, so a null declared value is left out.
#[derive(Serialize, Deserialize)]
struct Stored {
  value: toml::Value,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  declared: Option<toml::Value>,
  set_by: String,
  set_at: DateTime<Utc>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  expires_at: Option<DateTime<Utc>>,
}

const HEADER: &str = "# Runtime overrides, written by the application. Each one ends when it \
                      expires or when the configuration\n# files change the value it was set \
                      over. Remove one through the application rather than here.\n\n";

impl Overrides {
  /// The overrides kept at `path`, none when the file does not exist yet.
  pub fn load(path: impl Into<PathBuf>) -> Result<Self, Error> {
    let path = path.into();
    let Some(text) = files::read_optional_text(&path)? else {
      return Ok(Self {
        path: Some(path),
        items: BTreeMap::new(),
      });
    };
    let invalid = |message: String| Error::Overrides {
      path: path.clone(),
      message: format!("{message}; fix or delete the file to drop the overrides"),
    };
    let stored: BTreeMap<Key, Stored> =
      toml::from_str(&text).map_err(|e| invalid(e.to_string()))?;
    let items = stored
      .into_iter()
      .map(|(key, s)| {
        let value = tree::from_toml(s.value).map_err(invalid)?;
        let declared = match s.declared {
          Some(declared) => tree::from_toml(declared).map_err(invalid)?,
          None => Value::Null,
        };
        let item = Override {
          value,
          declared,
          set_by: s.set_by,
          set_at: s.set_at,
          expires_at: s.expires_at,
          until_restart: false,
        };
        Ok((key, item))
      })
      .collect::<Result<_, Error>>()?;
    Ok(Self {
      path: Some(path),
      items,
    })
  }

  /// The overrides in effect, by key.
  pub fn items(&self) -> &BTreeMap<Key, Override> {
    &self.items
  }

  /// Writes `items` to the file, then makes them the ones in effect, so nothing is in effect
  /// that a restart would lose.
  pub(crate) fn replace(&mut self, items: BTreeMap<Key, Override>) -> Result<(), Error> {
    self.save(&items)?;
    self.items = items;
    Ok(())
  }

  /// Makes `kept` the overrides in effect after others were dropped. A failed save is logged:
  /// the next load drops the others again.
  pub(crate) fn keep_only(&mut self, kept: BTreeMap<Key, Override>) {
    if let Err(e) = self.save(&kept) {
      warn!(error = %e, "the overrides file still lists dropped overrides");
    }
    self.items = kept;
  }

  fn save(&self, items: &BTreeMap<Key, Override>) -> Result<(), Error> {
    let Some(path) = &self.path else {
      return Ok(());
    };
    let mut stored = BTreeMap::new();
    for (key, o) in items.iter().filter(|(_, o)| !o.until_restart) {
      let value = to_toml(&o.value).ok_or_else(|| Error::Value {
        key: key.clone(),
        by: "a runtime override".into(),
        message: "the value has no TOML form".into(),
      })?;
      stored.insert(
        key.clone(),
        Stored {
          value,
          declared: to_toml(&o.declared),
          set_by: o.set_by.clone(),
          set_at: o.set_at,
          expires_at: o.expires_at,
        },
      );
    }
    let text = toml::to_string(&stored).map_err(|e| Error::Overrides {
      path: path.clone(),
      message: e.to_string(),
    })?;
    // an override may hold a secret
    write::write_atomic(
      path,
      format!("{HEADER}{text}").as_bytes(),
      Mode::KeepOr(0o600),
    )
  }

  /// The overrides that still apply over `declared`, the configuration below the runtime
  /// layer, at `now`, each as [`rebased`] over it, and the keys of those that no longer do, each
  /// with the reason.
  pub(crate) fn pruned(
    &self,
    schema: &Schema,
    declared: &Value,
    sources: &BTreeMap<Key, Source>,
    now: DateTime<Utc>,
  ) -> (BTreeMap<Key, Override>, Vec<(Key, String)>) {
    let mut kept = BTreeMap::new();
    let mut dropped = Vec::new();
    for (key, o) in &self.items {
      let current = tree::get(declared, key).unwrap_or(&Value::Null);
      let outcome = if o.expires_at.is_some_and(|t| t <= now) {
        Err("expired".to_string())
      } else if let Some(by) = sources.get(key).and_then(Source::fixed_by) {
        Err(format!("set by {by}"))
      } else {
        match schema.field(key) {
          Some(field) => rebased(field, o, current, &mut dropped),
          None => Err("not a configuration key".to_string()),
        }
      };
      match outcome {
        Ok(o) => {
          kept.insert(key.clone(), o);
        }
        Err(reason) => dropped.push((key.clone(), reason)),
      }
    }
    (kept, dropped)
  }
}

/// Why an override, or a key of a map entry it changes, gives way to the files.
const MOVED: &str = "the value it was set over changed";

/// `o`, an override of `field`, over `current`, the value below it now: as it is while that is
/// the value it was set over, else dropped. A user-keyed map's override instead keeps what it
/// changes and the files do not, set over `current`; each key of an entry that both change is
/// the files', and goes into `lost` with the reason.
fn rebased(
  field: &Field,
  o: &Override,
  current: &Value,
  lost: &mut Vec<(Key, String)>,
) -> Result<Override, String> {
  if same(current, &o.declared) {
    return Ok(o.clone());
  }
  if field.kind != Kind::Map {
    return Err(MOVED.to_string());
  }
  let value = merged_map(field, &field.key, &o.declared, &o.value, current, lost);
  if same(&value, current) {
    return Err(MOVED.to_string());
  }
  Ok(Override {
    value,
    declared: current.clone(),
    ..o.clone()
  })
}

/// The entries of `map`, a user-keyed map at `at`, with the runtime layer's changes from `base`
/// in `ours` kept over the files' in `theirs`, entry by entry.
fn merged_map<'a>(
  map: &Field,
  at: &str,
  base: &'a Value,
  ours: &'a Value,
  theirs: &'a Value,
  lost: &mut Vec<(Key, String)>,
) -> Value {
  let names: BTreeSet<&String> = [base, ours, theirs]
    .into_iter()
    .filter_map(Value::as_object)
    .flat_map(Map::keys)
    .collect();
  let mut merged = Map::new();
  for name in names {
    let entry = |side: &'a Value| side.get(name.as_str()).unwrap_or(&Value::Null);
    let at = format!("{at}.{name}");
    let value = merged_entry(map, &at, entry(base), entry(ours), entry(theirs), lost);
    if !value.is_null() {
      merged.insert(name.clone(), value);
    }
  }
  Value::Object(merged)
}

/// One entry of `map` merged as [`merged_map`] says: the side that changed it, or when both did
/// and both still have it, each of its keys on its own.
fn merged_entry<'a>(
  map: &Field,
  at: &str,
  base: &'a Value,
  ours: &'a Value,
  theirs: &'a Value,
  lost: &mut Vec<(Key, String)>,
) -> Value {
  if let Some(value) = settled(base, ours, theirs) {
    return value;
  }
  if map.entries.is_empty() || !ours.is_object() || !theirs.is_object() {
    lost.push((at.to_string(), MOVED.to_string()));
    return theirs.clone();
  }
  let mut entry = theirs.clone();
  for inner in &map.entries {
    let side = |side: &'a Value| tree::get(side, &inner.key).unwrap_or(&Value::Null);
    let (base, ours, theirs) = (side(base), side(ours), side(theirs));
    let at = format!("{at}.{}", inner.key);
    let value = if inner.kind == Kind::Map {
      merged_map(inner, &at, base, ours, theirs, lost)
    } else {
      settled(base, ours, theirs).unwrap_or_else(|| {
        lost.push((at, MOVED.to_string()));
        theirs.clone()
      })
    };
    if value.is_null() {
      tree::remove(&mut entry, &inner.key);
    } else {
      tree::set(&mut entry, &inner.key, value);
    }
  }
  entry
}

/// The value when only one side changed it from `base`, or both to the same; `None` when both
/// changed it apart.
fn settled(base: &Value, ours: &Value, theirs: &Value) -> Option<Value> {
  if same(ours, base) || same(ours, theirs) {
    Some(theirs.clone())
  } else if same(theirs, base) {
    Some(ours.clone())
  } else {
    None
  }
}

/// Whether two values are the same once nulls inside tables are left out, as the file stores
/// them.
fn same(a: &Value, b: &Value) -> bool {
  without_nulls(a) == without_nulls(b)
}

fn without_nulls(value: &Value) -> Value {
  match value {
    Value::Object(entries) => Value::Object(
      entries
        .iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| (k.clone(), without_nulls(v)))
        .collect(),
    ),
    Value::Array(items) => Value::Array(items.iter().map(without_nulls).collect()),
    other => other.clone(),
  }
}

/// A tree value as TOML, nulls inside tables left out; `None` for null itself, a null in a
/// list, or a number TOML cannot hold.
fn to_toml(value: &Value) -> Option<toml::Value> {
  Some(match value {
    Value::Null => return None,
    Value::Bool(b) => toml::Value::Boolean(*b),
    Value::Number(n) => match n.as_i64() {
      Some(i) => toml::Value::Integer(i),
      None if n.is_f64() => toml::Value::Float(n.as_f64()?),
      None => return None,
    },
    Value::String(s) => toml::Value::String(s.clone()),
    Value::Array(items) => toml::Value::Array(items.iter().map(to_toml).collect::<Option<_>>()?),
    Value::Object(entries) => toml::Value::Table(
      entries
        .iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| Some((k.clone(), to_toml(v)?)))
        .collect::<Option<_>>()?,
    ),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::example_schema;
  use chrono::Duration;
  use serde_json::json;

  fn item(value: Value, declared: Value, expires_at: Option<DateTime<Utc>>) -> Override {
    Override {
      value,
      declared,
      set_by: "admin".into(),
      set_at: Utc::now(),
      expires_at,
      until_restart: false,
    }
  }

  #[test]
  fn an_override_until_restart_is_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.toml");
    let mut overrides = Overrides::load(&path).unwrap();
    let volatile = Override {
      until_restart: true,
      ..item(json!(5000), json!(2242), None)
    };
    overrides
      .replace(BTreeMap::from([
        ("server.port".to_string(), volatile),
        ("debug".to_string(), item(json!(true), json!(false), None)),
      ]))
      .unwrap();
    assert_eq!(overrides.items().len(), 2, "both are in effect");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("server.port"), "{text}");
    let reloaded = Overrides::load(&path).unwrap();
    assert_eq!(reloaded.items().keys().collect::<Vec<_>>(), ["debug"]);
  }

  #[test]
  fn overrides_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.toml");
    let mut overrides = Overrides::load(&path).unwrap();
    assert!(overrides.items().is_empty());
    let in_an_hour = Utc::now() + Duration::hours(1);
    let items = BTreeMap::from([
      (
        "server.port".to_string(),
        item(json!(5000), json!(2242), Some(in_an_hour)),
      ),
      (
        "api_keys".to_string(),
        item(
          json!({ "mine": { "key": "k", "role": null } }),
          Value::Null,
          None,
        ),
      ),
    ]);
    overrides.replace(items).unwrap();

    let reloaded = Overrides::load(&path).unwrap();
    let port = &reloaded.items()["server.port"];
    assert_eq!(port.value, json!(5000));
    assert_eq!(port.declared, json!(2242));
    assert_eq!(port.expires_at, Some(in_an_hour));
    assert_eq!(reloaded.items()["api_keys"].declared, Value::Null);
    assert_eq!(
      reloaded.items()["api_keys"].value,
      json!({ "mine": { "key": "k" } })
    );

    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
      assert_eq!(mode, 0o600, "an override may hold a secret");
    }
  }

  #[test]
  fn a_broken_file_says_how_to_recover() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overrides.toml");
    std::fs::write(&path, "[\"x\"]\nvalue = ").unwrap();
    let err = Overrides::load(&path).unwrap_err().to_string();
    assert!(err.contains("fix or delete the file"), "{err}");
  }

  #[test]
  fn pruning_drops_expired_moved_and_fixed_overrides() {
    let schema = example_schema();
    let now = Utc::now();
    let overrides = Overrides {
      path: None,
      items: BTreeMap::from([
        ("server.port".into(), item(json!(1), json!(2242), None)),
        (
          "server.address".into(),
          item(json!("a"), json!("old"), None),
        ),
        (
          "instance_name".into(),
          item(json!("b"), json!("default"), Some(now)),
        ),
        ("debug".into(), item(json!(true), json!(false), None)),
        ("gone".into(), item(json!(1), Value::Null, None)),
        (
          "api_keys".into(),
          item(
            json!({}),
            json!({ "x": { "key": "k", "role": null } }),
            None,
          ),
        ),
      ]),
    };
    let declared = json!({
      "server": { "port": 2242, "address": "new" },
      "instance_name": "default",
      "debug": false,
      "api_keys": { "x": { "key": "k" } },
    });
    let sources = BTreeMap::from([(
      "debug".to_string(),
      Source::Env {
        var: "APP_DEBUG".into(),
      },
    )]);
    let (kept, dropped) = overrides.pruned(&schema, &declared, &sources, now);
    assert_eq!(
      kept.keys().collect::<Vec<_>>(),
      ["api_keys", "server.port"],
      "a null left out of a stored table still counts as the same value"
    );
    let dropped: BTreeMap<_, _> = dropped.into_iter().collect();
    assert_eq!(
      dropped["server.address"],
      "the value it was set over changed"
    );
    assert_eq!(dropped["instance_name"], "expired");
    assert_eq!(dropped["debug"], "set by APP_DEBUG");
    assert_eq!(dropped["gone"], "not a configuration key");
  }

  #[derive(Default, Serialize, crate::Cascade)]
  struct Seen {
    /// Who sees each share.
    #[cascade(user_keyed)]
    seen: BTreeMap<String, Vec<String>>,
  }

  #[test]
  fn a_map_override_keeps_what_the_files_do_not_change_too() {
    let schema = example_schema();
    let api_keys = schema.field("api_keys").unwrap();
    let mut lost = Vec::new();
    // an entry made at run time that a file defines later
    let made = item(
      json!({ "b": { "key": "mine", "role": "r" } }),
      json!({}),
      None,
    );
    let kept = rebased(
      api_keys,
      &made,
      &json!({ "b": { "key": "file" } }),
      &mut lost,
    )
    .unwrap();
    assert_eq!(
      kept.value,
      json!({ "b": { "key": "file", "role": "r" } }),
      "each key of an entry both define is the runtime layer's until the file sets it too"
    );
    assert_eq!(kept.declared, json!({ "b": { "key": "file" } }));
    assert_eq!(lost, [("api_keys.b.key".to_string(), MOVED.to_string())]);
    let cleared = item(
      json!({ "a": { "key": "k", "role": null } }),
      json!({ "a": { "key": "k", "role": "r" } }),
      None,
    );
    let kept = rebased(
      api_keys,
      &cleared,
      &json!({ "a": { "key": "k2", "role": "r" } }),
      &mut lost,
    )
    .unwrap();
    assert_eq!(
      kept.value,
      json!({ "a": { "key": "k2" } }),
      "a key cleared at run time stays out"
    );

    let schema = Schema::of::<Seen>();
    let seen = schema.field("seen").unwrap();
    let mut lost = Vec::new();
    let set = item(
      json!({ "music": ["a", "b"], "films": ["b"] }),
      json!({ "music": ["a"] }),
      None,
    );
    let kept = rebased(seen, &set, &json!({ "music": ["c"] }), &mut lost).unwrap();
    assert_eq!(
      kept.value,
      json!({ "music": ["c"], "films": ["b"] }),
      "an entry with no keys of its own is one value"
    );
    assert_eq!(lost, [("seen.music".to_string(), MOVED.to_string())]);

    let mut lost = Vec::new();
    let set = item(json!({ "music": ["a"] }), json!({}), None);
    assert_eq!(
      rebased(seen, &set, &json!({ "music": ["c"] }), &mut lost),
      Err(MOVED.to_string()),
      "an override with nothing left over the files is dropped"
    );
  }

  #[cfg(unix)]
  #[test]
  fn a_change_that_can_not_be_saved_is_not_made() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    if !crate::testing::read_only_dirs_hold(dir.path()) {
      return;
    }
    let mut overrides = Overrides::load(dir.path().join("overrides.toml")).unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = overrides.replace(BTreeMap::from([(
      "server.port".into(),
      item(json!(1), json!(2242), None),
    )]));
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
    assert!(overrides.items().is_empty());
  }
}
