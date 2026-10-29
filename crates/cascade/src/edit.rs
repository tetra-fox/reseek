//! Key-level edits to configuration files that keep their comments and formatting.

use std::collections::BTreeMap;

use serde_json::Value;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

use crate::schema::{REDACTED, Schema};
use crate::{Error, Key, tree};

/// Edits configuration files as text, keeping their comments and formatting.
#[derive(Clone, Copy, Debug)]
pub struct FileEditor<'a> {
  schema: &'a Schema,
}

impl<'a> FileEditor<'a> {
  /// An editor for files of the configuration `schema` describes.
  pub fn new(schema: &'a Schema) -> Self {
    Self { schema }
  }

  /// The file `name` holding `text`, with each key of `set` given its value and each key of
  /// `unset` removed. Only those keys change; tables are added where needed.
  ///
  /// For a secret key, `*****` (what a secret is shown as) leaves the value as it is and an
  /// empty string removes it. Null removes any key.
  pub fn patch(
    &self,
    name: &str,
    text: &str,
    set: &BTreeMap<Key, Value>,
    unset: &[Key],
  ) -> Result<String, Error> {
    let mut doc: DocumentMut = text
      .parse()
      .map_err(|e: toml_edit::TomlError| Error::Parse {
        file: name.to_string(),
        message: tree::parse_error(text, e.message(), e.span()),
      })?;
    let had_items = !doc.is_empty();
    let had_values = doc.iter().any(|(_, item)| item.is_value());
    let mut removals: Vec<&str> = unset.iter().map(String::as_str).collect();
    // the first table this edit adds at the top level, which goes below comments that end the
    // file
    let mut first_new: Option<String> = None;
    for (key, value) in set {
      let field = self
        .schema
        .field(key)
        .ok_or_else(|| Error::UnknownKey(key.clone()))?;
      let secret_kept = field.secret && value.as_str() == Some(REDACTED);
      if secret_kept {
        continue;
      }
      if value.is_null() || (field.secret && value.as_str() == Some("")) {
        removals.push(key);
        continue;
      }
      let item = to_item(key, value)?;
      let added = set_item(&mut doc, name, key, item)?;
      if let Some(top) = added {
        first_new.get_or_insert(top);
      }
    }
    for key in removals {
      let parts: Vec<&str> = key.split('.').collect();
      remove_key(doc.as_table_mut(), &parts);
    }
    let first_value = (!had_values)
      .then(|| {
        doc
          .iter()
          .find(|(_, item)| item.is_value())
          .map(|(k, _)| k.to_string())
      })
      .flatten();
    if let Some(key) = first_value {
      keep_banner_first(&mut doc, &key, had_items);
    } else if let Some(top) = first_new {
      move_trailing_comments(&mut doc, &top);
    }
    Ok(doc.to_string())
  }

  /// Whether `text` sets any secret to a non-empty value. Text that does not parse counts as
  /// holding one, so it is never written readable by others.
  pub(crate) fn holds_secrets(&self, text: &str) -> bool {
    let tree = match text.parse::<toml::Table>() {
      Ok(table) => tree::from_toml(toml::Value::Table(table)),
      Err(_) => return true,
    };
    match tree {
      Ok(tree) => self.schema.redact(&tree) != tree,
      Err(_) => true,
    }
  }
}

/// Puts `item` at the dotted `key`, adding the tables on the way as `[section]` headers.
/// Returns the top-level table's name when this added one.
fn set_item(
  doc: &mut DocumentMut,
  name: &str,
  key: &str,
  item: Item,
) -> Result<Option<String>, Error> {
  let parts: Vec<&str> = key.split('.').collect();
  let (last, tables) = parts.split_last().expect("split always yields one part");
  let mut node = doc.as_item_mut();
  let mut added_top = None;
  for (depth, part) in tables.iter().enumerate() {
    // indexing into a value as a table panics
    if node
      .get(part)
      .is_some_and(|i| !i.is_none() && !i.is_table_like())
    {
      return Err(Error::Value {
        key: key.to_string(),
        by: name.to_string(),
        message: format!("`{part}` is not a section in the file"),
      });
    }
    if node.is_table() && node.get(part).is_none_or(Item::is_none) {
      // a real `[section]` after its parent's, implicit while it only holds sections so it
      // prints no empty header; inside an inline table, indexing adds an inline table instead
      let mut table = Table::new();
      table.set_implicit(depth + 1 < tables.len());
      node[part] = Item::Table(table);
      if depth == 0 {
        added_top = Some(part.to_string());
      }
    }
    node = &mut node[part];
  }
  let in_inline = node.is_inline_table();
  let item = match item {
    // a table inside an inline table has to be inline too
    Item::Table(table) if in_inline => {
      Item::Value(toml_edit::Value::InlineTable(table.into_inline_table()))
    }
    Item::ArrayOfTables(tables) if in_inline => {
      Item::Value(toml_edit::Value::Array(tables.into_array()))
    }
    other => other,
  };
  let item = match (item, node.get(last).and_then(Item::as_value)) {
    (Item::Value(mut value), Some(old)) => {
      // keep the old value's spacing and trailing comment
      *value.decor_mut() = old.decor().clone();
      Item::Value(value)
    }
    (item, _) => item,
  };
  if node.get(last).is_none_or(Item::is_none)
    && item.is_table()
    && node.is_table()
    && tables.is_empty()
  {
    added_top = Some(last.to_string());
  }
  node[last] = item;
  Ok(added_top)
}

/// A file that ends in comments (often all of it, a file of commented-out examples) keeps them
/// at the end: they move in front of the first header the edit added under `top`.
fn move_trailing_comments(doc: &mut DocumentMut, top: &str) {
  let trailing = doc.trailing().as_str().unwrap_or_default().to_string();
  if trailing.trim().is_empty() {
    return;
  }
  let Some(table) = doc.get_mut(top).and_then(|item| first_header(item)) else {
    return;
  };
  let prefix = table
    .decor()
    .prefix()
    .and_then(|p| p.as_str())
    .unwrap_or_default()
    .to_string();
  // a blank line between the comments and the table
  let gap = if trailing.ends_with("\n\n") { "" } else { "\n" };
  table
    .decor_mut()
    .set_prefix(format!("{trailing}{gap}{prefix}"));
  doc.set_trailing("");
}

/// A value added at the top level of a file that had none prints above everything else, so the
/// comments that opened the file move in front of it: the whole text of a file that was only
/// comments, else what comes before the first blank line above the first table.
fn keep_banner_first(doc: &mut DocumentMut, key: &str, had_items: bool) {
  let banner = if had_items {
    let Some(table) = first_printed_table(doc) else {
      return;
    };
    let prefix = table
      .decor()
      .prefix()
      .and_then(|p| p.as_str())
      .unwrap_or_default()
      .to_string();
    let Some(end) = prefix.rfind("\n\n") else {
      return;
    };
    table.decor_mut().set_prefix(prefix[end + 2..].to_string());
    prefix[..end + 2].to_string()
  } else {
    let trailing = doc.trailing().as_str().unwrap_or_default().to_string();
    doc.set_trailing("");
    let gap = if trailing.is_empty() || trailing.ends_with("\n\n") {
      ""
    } else {
      "\n"
    };
    format!("{trailing}{gap}")
  };
  if let Some(mut key) = doc.as_table_mut().key_mut(key) {
    let decor = key.leaf_decor_mut();
    let existing = decor
      .prefix()
      .and_then(|p| p.as_str())
      .unwrap_or_default()
      .to_string();
    decor.set_prefix(format!("{banner}{existing}"));
  }
}

/// The table whose header comes first in the file.
fn first_printed_table(doc: &mut DocumentMut) -> Option<&mut Table> {
  let first = doc
    .iter()
    .filter_map(|(key, item)| Some((header_position(item)?, key.to_string())))
    .min()?
    .1;
  doc.get_mut(&first).and_then(first_header)
}

/// Where the first header at or under `item` is in the file it was read from.
fn header_position(item: &Item) -> Option<isize> {
  match item {
    Item::Table(table) if table.is_implicit() => table
      .iter()
      .filter_map(|(_, inner)| header_position(inner))
      .min(),
    Item::Table(table) => table.position(),
    Item::ArrayOfTables(tables) => tables.iter().next().and_then(Table::position),
    _ => None,
  }
}

/// The first table at or under `item` that prints a header; an implicit table prints none.
fn first_header(item: &mut Item) -> Option<&mut Table> {
  match item {
    Item::Table(table) => {
      if table.is_implicit() {
        table.iter_mut().find_map(|(_, inner)| first_header(inner))
      } else {
        Some(table)
      }
    }
    Item::ArrayOfTables(tables) => tables.iter_mut().next(),
    _ => None,
  }
}

fn remove_key(table: &mut dyn toml_edit::TableLike, parts: &[&str]) {
  match parts {
    [last] => {
      table.remove(last);
    }
    [first, rest @ ..] => {
      if let Some(inner) = table.get_mut(first).and_then(Item::as_table_like_mut) {
        remove_key(inner, rest);
      }
    }
    [] => {}
  }
}

/// A tree value as a TOML item: tables of tables and lists of tables become `[section]` and
/// `[[section]]` blocks, everything else a value.
fn to_item(key: &str, value: &Value) -> Result<Item, Error> {
  match value {
    Value::Object(entries) => {
      let mut table = Table::new();
      for (k, v) in entries {
        if v.is_null() {
          continue;
        }
        table.insert(k, to_item(key, v)?);
      }
      let only_tables = !table.is_empty()
        && table
          .iter()
          .all(|(_, item)| item.is_table() || item.is_array_of_tables());
      table.set_implicit(only_tables);
      Ok(Item::Table(table))
    }
    Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_object) => {
      let mut tables = ArrayOfTables::new();
      for item in items {
        match to_item(key, item)? {
          Item::Table(table) => tables.push(table),
          _ => unreachable!("an object becomes a table"),
        }
      }
      Ok(Item::ArrayOfTables(tables))
    }
    other => Ok(Item::Value(to_value(key, other)?)),
  }
}

fn to_value(key: &str, value: &Value) -> Result<toml_edit::Value, Error> {
  let unrepresentable = |message: String| Error::Value {
    key: key.to_string(),
    by: "the edit".to_string(),
    message,
  };
  Ok(match value {
    Value::Bool(b) => (*b).into(),
    Value::String(s) => s.as_str().into(),
    Value::Number(n) => match n.as_i64() {
      Some(i) => i.into(),
      None if n.is_f64() => n
        .as_f64()
        .ok_or_else(|| unrepresentable(format!("{n} has no TOML form")))?
        .into(),
      None => return Err(unrepresentable(format!("{n} is too large for TOML"))),
    },
    Value::Array(items) => {
      let mut array = toml_edit::Array::new();
      for item in items {
        array.push(to_value(key, item)?);
      }
      toml_edit::Value::Array(array)
    }
    Value::Object(entries) => {
      let mut table = toml_edit::InlineTable::new();
      for (k, v) in entries {
        if !v.is_null() {
          table.insert(k, to_value(key, v)?);
        }
      }
      toml_edit::Value::InlineTable(table)
    }
    Value::Null => return Err(unrepresentable("a list can not hold null".to_string())),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::example_schema;
  use serde_json::json;

  fn patch(text: &str, set: &[(&str, Value)], unset: &[&str]) -> Result<String, Error> {
    let schema = example_schema();
    let set = set
      .iter()
      .map(|(k, v)| (k.to_string(), v.clone()))
      .collect();
    let unset: Vec<Key> = unset.iter().map(|k| k.to_string()).collect();
    FileEditor::new(&schema).patch("config.toml", text, &set, &unset)
  }

  #[test]
  fn only_the_named_keys_change() {
    let text = "# the server\n[server]\nport = 2242   # the usual port\naddress = \"x\" # where\n\n[shares]\n# none yet\ndirectories = []\n";
    let edited = patch(text, &[("server.port", json!(5000))], &[]).unwrap();
    assert_eq!(
      edited,
      text.replace("2242", "5000"),
      "comments and spacing stay"
    );

    let edited = patch(text, &[], &["server.address"]).unwrap();
    assert_eq!(
      edited,
      "# the server\n[server]\nport = 2242   # the usual port\n\n[shares]\n# none yet\ndirectories = []\n"
    );
  }

  #[test]
  fn new_sections_go_below_comments_that_end_the_file() {
    let banner = "# Written by the application\n\n# [server]\n# port = 2242\n";
    let edited = patch(banner, &[("server.port", json!(5353))], &[]).unwrap();
    assert!(edited.starts_with(banner), "{edited}");
    assert!(edited.ends_with("\n[server]\nport = 5353\n"), "{edited}");

    let edited = patch(banner, &[("shares.cache.workers", json!(2))], &[]).unwrap();
    assert!(edited.starts_with(banner), "{edited}");
    assert!(
      edited.ends_with("\n[shares.cache]\nworkers = 2\n"),
      "{edited}"
    );
    assert!(
      !edited.contains("\n[shares]\n"),
      "no empty parent header: {edited}"
    );

    let text = "[server]\nport = 1\n\n[flags]\nvolatile = true\n# the end\n";
    let edited = patch(text, &[("shares.cache.workers", json!(2))], &[]).unwrap();
    assert!(
      edited.ends_with("# the end\n\n[shares.cache]\nworkers = 2\n"),
      "{edited}"
    );
    let parsed: toml::Table = edited.parse().unwrap();
    assert_eq!(parsed["flags"]["volatile"].as_bool(), Some(true));
    assert_eq!(parsed["shares"]["cache"]["workers"].as_integer(), Some(2));
  }

  #[test]
  fn a_top_level_value_goes_below_the_banner() {
    let banner = "# Written by the application\n# [server]\n# port = 2242\n";
    let edited = patch(banner, &[("debug", json!(true))], &[]).unwrap();
    assert_eq!(edited, format!("{banner}\ndebug = true\n"));

    let text = "# mine\n\n# the server\n[server]\nport = 1\n";
    let edited = patch(text, &[("debug", json!(true))], &[]).unwrap();
    assert_eq!(
      edited,
      "# mine\n\ndebug = true\n# the server\n[server]\nport = 1\n"
    );

    let text = "debug = false\n# mine\n[server]\nport = 1\n";
    let edited = patch(text, &[("instance_name", json!("x"))], &[]).unwrap();
    assert_eq!(
      edited,
      "debug = false\ninstance_name = \"x\"\n# mine\n[server]\nport = 1\n"
    );
  }

  #[test]
  fn under_an_inline_table_new_tables_are_inline_too() {
    let edited = patch(
      "server = { port = 1 }\n",
      &[("server.address", json!("a"))],
      &[],
    )
    .unwrap();
    let parsed: toml::Table = edited.parse().unwrap();
    assert_eq!(parsed["server"]["port"].as_integer(), Some(1));
    assert_eq!(parsed["server"]["address"].as_str(), Some("a"));

    let edited = patch(
      "shares = { directories = [] }\n",
      &[("shares.cache.workers", json!(3))],
      &[],
    )
    .unwrap();
    let parsed: toml::Table = edited.parse().unwrap();
    assert_eq!(parsed["shares"]["cache"]["workers"].as_integer(), Some(3));
  }

  #[test]
  fn editing_under_a_value_is_refused() {
    let e = patch("server = 5\n", &[("server.port", json!(1))], &[]).unwrap_err();
    assert!(e.to_string().contains("`server` is not a section"), "{e}");
    let e = patch("", &[("server.nope", json!(1))], &[]).unwrap_err();
    assert!(matches!(e, Error::UnknownKey(k) if k == "server.nope"));
    let e = patch("[server\n", &[], &[]).unwrap_err();
    assert!(matches!(e, Error::Parse { .. }));
  }

  #[test]
  fn a_typo_can_be_removed() {
    let edited = patch("[server]\nprot = 1\nport = 2\n", &[], &["server.prot"]).unwrap();
    assert_eq!(edited, "[server]\nport = 2\n");
  }

  #[test]
  fn secrets_keep_their_value_unless_changed() {
    let text = "[server]\npassword = \"hunter2\"\n";
    let kept = patch(text, &[("server.password", json!(REDACTED))], &[]).unwrap();
    assert_eq!(kept, text);
    let cleared = patch(text, &[("server.password", json!(""))], &[]).unwrap();
    assert_eq!(cleared, "[server]\n");
    let changed = patch(text, &[("server.password", json!("new"))], &[]).unwrap();
    assert_eq!(changed, "[server]\npassword = \"new\"\n");
    let removed = patch(text, &[("server.password", Value::Null)], &[]).unwrap();
    assert_eq!(removed, "[server]\n");
    let blank = patch(
      "[server]\nusername = \"a\"\n",
      &[("server.username", json!(""))],
      &[],
    );
    assert_eq!(
      blank.unwrap(),
      "[server]\nusername = \"\"\n",
      "an empty string is a value for a key that is not secret"
    );
  }

  #[test]
  fn maps_and_lists_of_tables_are_written_as_sections() {
    let edited = patch(
      "# keys\n",
      &[
        (
          "api_keys",
          json!({ "mine": { "key": "k1", "role": "admin" }, "my.other": { "key": "k2", "role": null } }),
        ),
        ("webhooks", json!([{ "url": "http://a" }, { "url": "http://b", "token": "t" }])),
        ("shares.limits", json!([1, 2])),
      ],
      &[],
    )
    .unwrap();
    let parsed: toml::Table = edited.parse().unwrap();
    assert_eq!(parsed["api_keys"]["mine"]["role"].as_str(), Some("admin"));
    assert_eq!(parsed["api_keys"]["my.other"]["key"].as_str(), Some("k2"));
    assert!(parsed["api_keys"]["my.other"].get("role").is_none());
    assert_eq!(parsed["webhooks"].as_array().unwrap().len(), 2);
    assert!(edited.contains("[api_keys.mine]"), "{edited}");
    assert!(edited.contains("[[webhooks]]"), "{edited}");
    assert!(!edited.contains("\n[api_keys]\n"), "{edited}");
    assert!(edited.starts_with("# keys\n"), "{edited}");

    let replaced = patch(&edited, &[("api_keys", json!({}))], &[]).unwrap();
    let parsed: toml::Table = replaced.parse().unwrap();
    assert!(parsed["api_keys"].as_table().unwrap().is_empty());
  }

  #[test]
  fn secrets_are_found_anywhere() {
    let schema = example_schema();
    let editor = FileEditor::new(&schema);
    assert!(!editor.holds_secrets("[server]\nport = 1\npassword = \"\"\n"));
    assert!(editor.holds_secrets("[server]\npassword = \"x\"\n"));
    assert!(editor.holds_secrets("[api_keys.a]\nkey = \"k\"\n"));
    assert!(editor.holds_secrets("[[webhooks]]\nurl = \"u\"\ntoken = \"t\"\n"));
    assert!(
      editor.holds_secrets("[server\n"),
      "unparsable text counts as secret"
    );
  }
}
