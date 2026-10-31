//! Every leaf key of a configuration with what is known about it, built from the derive's
//! description and the configuration's defaults; diffs and redaction over it.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use crate::describe::{Apply, Cascade, Inner, Kind, Node};
use crate::{Invalid, Key, tree};

/// What a secret value is shown as.
pub const REDACTED: &str = "*****";

/// One leaf key of a configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
  /// the dotted key, e.g. `web.authentication.password`
  pub key: Key,
  /// what kind of value the key holds
  pub kind: Kind,
  /// the value when no layer sets the key, null for none
  pub default: Value,
  /// the environment variable that sets the key, the name shown to users
  pub env: Option<String>,
  /// more names for the same variable; when several are set the first alias wins, then the
  /// next, and `env` last
  pub env_aliases: Vec<String>,
  /// the long command-line flag that sets the key, e.g. `--debug`
  pub cli: Option<String>,
  /// the short form of the flag, e.g. `d` for `-d`
  pub short: Option<char>,
  /// the flag turns the boolean off: given alone it sets `false`, given a value it sets the
  /// opposite of that value
  pub negated: bool,
  /// when a change to the value takes effect
  pub apply: Apply,
  /// shown as `*****`, never logged
  pub secret: bool,
  /// the field's help text
  pub doc: String,
  /// the names the value is one of, `None` when it is not one of a fixed set
  pub choices: Option<Vec<String>>,
  /// the fields of each element of a list of tables or entry of a user-keyed map, with keys
  /// relative to the element
  pub entries: Vec<Field>,
}

impl Field {
  fn from_node(node: Node, defaults: &Value) -> Self {
    let default = tree::get(defaults, &node.key)
      .cloned()
      .unwrap_or(Value::Null);
    Self {
      key: node.key,
      kind: node.kind,
      default,
      env: node.env,
      env_aliases: node.env_aliases,
      cli: node.long,
      short: node.short,
      negated: node.negated,
      apply: node.apply.unwrap_or_default(),
      secret: node.secret,
      doc: node.doc,
      choices: node
        .choices
        .map(|names| names.iter().map(|n| n.to_string()).collect()),
      entries: node
        .entries
        .into_iter()
        .map(|entry| Field::from_node(entry, &Value::Null))
        .collect(),
    }
  }

  /// Whether the field, or anything in its entries, is secret.
  fn may_hold_secret(&self) -> bool {
    self.secret || self.entries.iter().any(Field::may_hold_secret)
  }

  /// The variable names that set the key, in the order they win when several are set.
  pub fn env_names(&self) -> impl Iterator<Item = &str> {
    self.env_aliases.iter().chain(&self.env).map(String::as_str)
  }
}

/// Every leaf key of a configuration type, from `#[derive(Cascade)]` and the type's `Default`.
#[derive(Clone, Debug)]
pub struct Schema {
  fields: Vec<Field>,
  index: BTreeMap<Key, usize>,
  /// keys that hold other keys, such as `web` and `web.authentication`
  sections: BTreeSet<Key>,
}

impl Schema {
  /// The schema of `T`.
  ///
  /// # Panics
  ///
  /// When `T` is not a struct deriving `Cascade`, its default does not serialize, or two fields
  /// share a key, an env var or a flag. These are mistakes in the type, found the first time
  /// its schema is built.
  pub fn of<T: Cascade + Default + Serialize>() -> Self {
    let Inner::Section(nodes) = T::shape().0 else {
      panic!("a configuration's root is a struct deriving Cascade");
    };
    let defaults = serde_json::to_value(T::default())
      .unwrap_or_else(|e| panic!("the configuration's default does not serialize: {e}"));
    let fields: Vec<Field> = nodes
      .into_iter()
      .map(|node| Field::from_node(node, &defaults))
      .collect();

    let mut index = BTreeMap::new();
    let mut sections = BTreeSet::new();
    let mut names = BTreeSet::new();
    for (i, field) in fields.iter().enumerate() {
      assert!(
        index.insert(field.key.clone(), i).is_none(),
        "two fields have the key `{}`",
        field.key
      );
      let mut parts: Vec<&str> = field.key.split('.').collect();
      parts.pop();
      for end in 1..=parts.len() {
        sections.insert(parts[..end].join("."));
      }
      let flags = field
        .cli
        .iter()
        .cloned()
        .chain(field.short.map(|c| format!("-{c}")));
      for name in field.env_names().map(str::to_string).chain(flags) {
        assert!(names.insert(name.clone()), "`{name}` sets two fields");
      }
    }
    if let Some(key) = index.keys().find(|key| sections.contains(*key)) {
      panic!("`{key}` is both a value and a section");
    }
    Self {
      fields,
      index,
      sections,
    }
  }

  /// Every leaf key, in the struct's field order.
  pub fn fields(&self) -> &[Field] {
    &self.fields
  }

  /// The field with this dotted key.
  pub fn field(&self, key: &str) -> Option<&Field> {
    self.index.get(key).map(|&i| &self.fields[i])
  }

  /// Whether the key holds other keys.
  pub(crate) fn is_section(&self, key: &str) -> bool {
    self.sections.contains(key)
  }

  /// The field the long command-line flag sets.
  pub(crate) fn by_flag(&self, flag: &str) -> Option<&Field> {
    self.fields.iter().find(|f| f.cli.as_deref() == Some(flag))
  }

  /// The field a path into the configuration falls under, such as `shares.directories` for
  /// `shares.directories[2]` or `web.authentication.api_keys` for one of its entries.
  pub(crate) fn field_under(&self, path: &str) -> Option<&Field> {
    let mut key = path.split('[').next().unwrap_or(path);
    loop {
      if let Some(field) = self.field(key) {
        return Some(field);
      }
      key = &key[..key.rfind('.')?];
    }
  }

  /// Whether a path into the configuration may hold a secret, so a message about its value
  /// must not quote it.
  pub(crate) fn may_hold_secret(&self, path: &str) -> bool {
    self.field_under(path).is_some_and(Field::may_hold_secret)
  }

  /// The leaf keys whose values differ between `old` and `new`, compared as serialized. A list
  /// or user-keyed map is one value.
  ///
  /// # Panics
  ///
  /// When either value does not serialize, which for a type whose default does takes a
  /// `Serialize` of its own that fails.
  pub fn diff<T: Serialize>(&self, old: &T, new: &T) -> Vec<Key> {
    match (serde_json::to_value(old), serde_json::to_value(new)) {
      (Ok(old), Ok(new)) => self.diff_trees(&old, &new),
      (Err(e), _) | (_, Err(e)) => panic!("the configuration does not serialize: {e}"),
    }
  }

  pub(crate) fn diff_trees(&self, old: &Value, new: &Value) -> Vec<Key> {
    self
      .changed_fields(old, new)
      .map(|f| f.key.clone())
      .collect()
  }

  /// The fields whose values differ between two configuration trees.
  pub(crate) fn changed_fields<'a>(
    &'a self,
    old: &'a Value,
    new: &'a Value,
  ) -> impl Iterator<Item = &'a Field> {
    self
      .fields
      .iter()
      .filter(|f| tree::get(old, &f.key) != tree::get(new, &f.key))
  }

  /// The configuration with every secret value replaced by `*****`. A secret that is null or
  /// empty stays as it is, so it shows as not set.
  pub fn redact(&self, values: &Value) -> Value {
    let mut values = values.clone();
    redact_fields(&self.fields, &mut values);
    values
  }

  /// The value of `key` as it may be shown or logged.
  pub(crate) fn redact_value(&self, key: &str, value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(field) = self.field(key) {
      redact_field(field, &mut value);
    }
    value
  }
}

fn redact_fields(fields: &[Field], values: &mut Value) {
  for field in fields {
    if let Some(value) = get_mut(values, &field.key) {
      redact_field(field, value);
    }
  }
}

fn redact_field(field: &Field, value: &mut Value) {
  if field.secret {
    hide(value);
    return;
  }
  if field.entries.is_empty() {
    return;
  }
  match value {
    Value::Array(items) => items
      .iter_mut()
      .for_each(|item| redact_fields(&field.entries, item)),
    Value::Object(entries) if field.kind == Kind::Map => entries
      .values_mut()
      .for_each(|entry| redact_fields(&field.entries, entry)),
    _ => {}
  }
}

/// Puts back the secrets of `value`, a new value of `field`, which is a list of tables or a
/// user-keyed map, wherever it shows them as `*****`: what a client read comes with its secrets
/// hidden, and it sends the whole value back when it changes one entry. A map's entry keeps its
/// secret from the entry of the same name in `current`, the value in effect. Nothing can tell
/// whose secret a renamed entry or a list element shows, so those are refused, and the text
/// `*****` never takes the place of a secret.
pub(crate) fn keep_secrets(
  field: &Field,
  value: &mut Value,
  current: Option<&Value>,
) -> Result<(), Invalid> {
  kept_in(field, &field.key, value, current).map_err(|path| {
    Invalid::new(
      field.key.clone(),
      format!("The field {path} is hidden and has no value to keep here. Enter it again."),
    )
  })
}

/// [`keep_secrets`] for the value at `path`, whose error is the path of the hidden value.
fn kept_in(
  field: &Field,
  path: &str,
  value: &mut Value,
  current: Option<&Value>,
) -> Result<(), String> {
  if field.secret {
    if value.as_str() != Some(REDACTED) {
      return Ok(());
    }
    *value = current
      .filter(|real| !real.is_null())
      .ok_or_else(|| path.to_string())?
      .clone();
    return Ok(());
  }
  match value {
    Value::Object(entries) if field.kind == Kind::Map => {
      for (name, entry) in entries {
        let kept = current.and_then(|map| map.get(name.as_str()));
        kept_in_entry(field, &format!("{path}.{name}"), entry, kept)?;
      }
    }
    Value::Array(items) => {
      for (index, item) in items.iter_mut().enumerate() {
        kept_in_entry(field, &format!("{path}[{index}]"), item, None)?;
      }
    }
    _ => {}
  }
  Ok(())
}

/// [`kept_in`] for each field of `entry`, an element of `field`.
fn kept_in_entry(
  field: &Field,
  path: &str,
  entry: &mut Value,
  current: Option<&Value>,
) -> Result<(), String> {
  for inner in &field.entries {
    if let Some(value) = get_mut(entry, &inner.key) {
      let kept = current.and_then(|entry| tree::get(entry, &inner.key));
      kept_in(inner, &format!("{path}.{}", inner.key), value, kept)?;
    }
  }
  Ok(())
}

fn hide(value: &mut Value) {
  match value {
    Value::Null => {}
    Value::String(s) if s.is_empty() => {}
    Value::Array(items) => items.iter_mut().for_each(hide),
    Value::Object(entries) => entries.values_mut().for_each(hide),
    _ => *value = Value::String(REDACTED.to_string()),
  }
}

fn get_mut<'a>(tree: &'a mut Value, key: &str) -> Option<&'a mut Value> {
  key
    .split('.')
    .try_fold(tree, |node, part| node.as_object_mut()?.get_mut(part))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{Example, example_schema};
  use serde_json::json;

  #[test]
  fn the_derive_describes_every_leaf() {
    let schema = example_schema();
    let keys: Vec<&str> = schema.fields().iter().map(|f| f.key.as_str()).collect();
    assert_eq!(
      keys,
      [
        "debug",
        "instance_name",
        "server.address",
        "server.port",
        "server.username",
        "server.password",
        "server.timeout",
        "server.buffer",
        "shares.directories",
        "shares.limits",
        "shares.cache.workers",
        "shares.cache.mode",
        "api_keys",
        "webhooks",
        "flags.no_connect",
        "flags.volatile",
      ]
    );

    let password = schema.field("server.password").unwrap();
    assert!(password.secret);
    assert_eq!(password.apply, Apply::Reconnect);
    assert_eq!(password.env.as_deref(), Some("APP_SERVER_PASSWORD"));
    assert_eq!(password.env_aliases, ["NEW_SERVER_PASSWORD"]);
    assert_eq!(password.default, Value::Null);
    assert_eq!(password.doc, "The password to log in with.");

    let debug = schema.field("debug").unwrap();
    assert_eq!(debug.cli.as_deref(), Some("--debug"));
    assert_eq!(debug.short, Some('d'));
    assert_eq!(debug.kind, Kind::Bool);
    assert_eq!(debug.apply, Apply::Restart);

    let port = schema.field("server.port").unwrap();
    assert_eq!(port.kind, Kind::Integer);
    assert_eq!(port.default, json!(2242));
    assert_eq!(port.apply, Apply::Reconnect, "inherited from the section");
    assert_eq!(
      port.env_names().collect::<Vec<_>>(),
      ["NEW_PORT", "NEW_SERVER_PORT", "APP_SERVER_PORT"],
      "the field's own alias first, then the prefix rule's, then the main name"
    );
    assert_eq!(
      schema.field("server.timeout").unwrap().apply,
      Apply::Live,
      "a field's own class beats its section's"
    );
    assert_eq!(schema.field("server.timeout").unwrap().kind, Kind::Duration);
    assert_eq!(schema.field("server.buffer").unwrap().kind, Kind::Size);
    assert_eq!(
      schema.field("shares.directories").unwrap().kind,
      Kind::List(Box::new(Kind::String))
    );
    assert_eq!(
      schema.field("flags.volatile").unwrap().apply,
      Apply::Restart
    );
    assert_eq!(
      schema.field("shares.cache.mode").unwrap().kind,
      Kind::String
    );

    let api_keys = schema.field("api_keys").unwrap();
    assert_eq!(api_keys.kind, Kind::Map);
    assert_eq!(
      api_keys
        .entries
        .iter()
        .map(|f| (f.key.as_str(), f.secret))
        .collect::<Vec<_>>(),
      [("key", true), ("role", false)]
    );
    let webhooks = schema.field("webhooks").unwrap();
    assert_eq!(webhooks.kind, Kind::List(Box::new(Kind::Table)));
    assert_eq!(webhooks.entries.len(), 2);

    assert!(schema.is_section("shares") && schema.is_section("shares.cache"));
    assert!(!schema.is_section("shares.cache.workers"));
    assert_eq!(schema.by_flag("--debug").unwrap().key, "debug");
    assert_eq!(
      schema.field_under("shares.directories[2]").unwrap().key,
      "shares.directories"
    );
    assert_eq!(
      schema.field_under("api_keys.mine.key").unwrap().key,
      "api_keys"
    );
    assert!(schema.field_under("nope.x").is_none());
  }

  #[derive(Default, Serialize, crate::Cascade)]
  struct TwoVars {
    #[cascade(env = "SAME")]
    a: u8,
    #[cascade(env = "OTHER", env_alias = "SAME")]
    b: u8,
  }

  #[test]
  #[should_panic(expected = "`SAME` sets two fields")]
  fn a_variable_for_two_fields_is_a_mistake() {
    Schema::of::<TwoVars>();
  }

  #[derive(Default, Serialize, crate::Cascade)]
  struct TwoFlags {
    #[cascade(cli = "-a|--alpha")]
    a: bool,
    #[cascade(cli = "-a|--again")]
    b: bool,
  }

  #[test]
  #[should_panic(expected = "`-a` sets two fields")]
  fn a_short_flag_for_two_fields_is_a_mistake() {
    Schema::of::<TwoFlags>();
  }

  #[derive(Default, Serialize, crate::Cascade)]
  struct Inner {
    a: u8,
  }

  #[derive(Default, Serialize, crate::Cascade)]
  struct EnvOnSection {
    #[cascade(env = "INNER")]
    inner: Inner,
  }

  #[test]
  #[should_panic(expected = "`inner` is a section")]
  fn a_section_takes_no_variable() {
    Schema::of::<EnvOnSection>();
  }

  #[test]
  fn diff_names_changed_leaves_and_whole_lists() {
    let schema = example_schema();
    let old = Example::default();
    let mut new = Example::default();
    assert!(schema.diff(&old, &new).is_empty());
    new.server.port = 1;
    new.shares.directories.push("/music".into());
    new
      .api_keys
      .insert("mine".into(), crate::testing::ApiKey::default());
    assert_eq!(
      schema.diff(&old, &new),
      ["server.port", "shares.directories", "api_keys"]
    );
  }

  #[test]
  fn redaction_hides_set_secrets_only() {
    let schema = example_schema();
    let mut example = Example::default();
    example.server.password = Some("hunter2".into());
    example.api_keys.insert(
      "mine".into(),
      crate::testing::ApiKey {
        key: "abcdefghijklmnop".into(),
        role: "readonly".into(),
      },
    );
    example.api_keys.insert("empty".into(), Default::default());
    example.webhooks.push(crate::testing::Webhook {
      url: "http://x".into(),
      token: Some("t".into()),
    });
    let redacted = schema.redact(&serde_json::to_value(&example).unwrap());
    assert_eq!(redacted["server"]["password"], json!(REDACTED));
    assert_eq!(redacted["server"]["username"], json!("guest"));
    assert_eq!(redacted["api_keys"]["mine"]["key"], json!(REDACTED));
    assert_eq!(redacted["api_keys"]["mine"]["role"], json!("readonly"));
    assert_eq!(redacted["api_keys"]["empty"]["key"], json!(""));
    assert_eq!(redacted["webhooks"][0]["token"], json!(REDACTED));
    assert_eq!(redacted["webhooks"][0]["url"], json!("http://x"));

    example.server.password = None;
    let redacted = schema.redact(&serde_json::to_value(&example).unwrap());
    assert_eq!(redacted["server"]["password"], Value::Null);
    assert_eq!(
      schema.redact_value("server.password", &json!(["a", ""])),
      json!([REDACTED, ""])
    );
  }
}
