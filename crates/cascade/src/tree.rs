//! The merged configuration as a JSON tree addressed by dotted keys, and turning env var,
//! command-line and TOML values into tree values.

use std::ops::Range;

use serde_json::{Map, Number, Value};

use crate::describe::Kind;

/// The value at a dotted key, `None` where the tree has nothing there.
pub(crate) fn get<'a>(tree: &'a Value, key: &str) -> Option<&'a Value> {
  key
    .split('.')
    .try_fold(tree, |node, part| node.as_object()?.get(part))
}

/// [`get`], to change the value.
pub(crate) fn get_mut<'a>(tree: &'a mut Value, key: &str) -> Option<&'a mut Value> {
  key
    .split('.')
    .try_fold(tree, |node, part| node.as_object_mut()?.get_mut(part))
}

/// Sets the value at a dotted key, turning anything on the way that is not a table (a section
/// that is `None`) into one.
pub(crate) fn set(tree: &mut Value, key: &str, value: Value) {
  let mut node = tree;
  let mut parts = key.split('.').peekable();
  while let Some(part) = parts.next() {
    if !node.is_object() {
      *node = Value::Object(Map::new());
    }
    let table = node.as_object_mut().expect("made a table above");
    if parts.peek().is_none() {
      table.insert(part.to_string(), value);
      return;
    }
    node = table.entry(part).or_insert(Value::Null);
  }
}

/// A value from an env var or the command line, read as `kind`. A list's elements come from
/// `split`, each read as the list's element kind.
pub(crate) fn coerce(raw: &str, kind: &Kind, split: Option<char>) -> Result<Value, String> {
  Ok(match kind {
    Kind::Bool => match raw.to_ascii_lowercase().as_str() {
      "true" => Value::Bool(true),
      "false" => Value::Bool(false),
      _ => return Err(format!("expected true or false, got {raw:?}")),
    },
    Kind::Integer => raw
      .parse::<i64>()
      .map(Value::from)
      .or_else(|_| raw.parse::<u64>().map(Value::from))
      .map_err(|_| format!("expected an integer, got {raw:?}"))?,
    Kind::Float => raw
      .parse::<f64>()
      .ok()
      .and_then(Number::from_f64)
      .map(Value::Number)
      .ok_or_else(|| format!("expected a number, got {raw:?}"))?,
    // a plain byte count stays a number, anything else is the size type's to parse
    Kind::Size => raw
      .parse::<u64>()
      .map(Value::from)
      .unwrap_or_else(|_| Value::String(raw.to_string())),
    Kind::String | Kind::Duration => Value::String(raw.to_string()),
    Kind::List(item) => {
      let Some(split) = split else {
        return coerce(raw, item, None);
      };
      // an empty variable is an empty list, not a list of one empty element
      if raw.is_empty() {
        return Ok(Value::Array(Vec::new()));
      }
      Value::Array(
        raw
          .split(split)
          .map(|element| coerce(element, item, None))
          .collect::<Result<_, _>>()?,
      )
    }
    Kind::Map | Kind::Table => {
      return Err("a table can only be set in a file".to_string());
    }
  })
}

/// A TOML value as a tree value. Dates and times become their TOML text.
pub(crate) fn from_toml(value: toml::Value) -> Result<Value, String> {
  Ok(match value {
    toml::Value::String(s) => Value::String(s),
    toml::Value::Integer(i) => Value::from(i),
    toml::Value::Float(f) => Number::from_f64(f)
      .map(Value::Number)
      .ok_or_else(|| format!("{f} is not a finite number"))?,
    toml::Value::Boolean(b) => Value::Bool(b),
    toml::Value::Datetime(d) => Value::String(d.to_string()),
    toml::Value::Array(items) => {
      Value::Array(items.into_iter().map(from_toml).collect::<Result<_, _>>()?)
    }
    toml::Value::Table(table) => Value::Object(
      table
        .into_iter()
        .map(|(k, v)| Ok((k, from_toml(v)?)))
        .collect::<Result<_, String>>()?,
    ),
  })
}

/// A TOML error as one line that says where it is.
pub(crate) fn parse_error(text: &str, message: &str, span: Option<Range<usize>>) -> String {
  let Some(before) = span.and_then(|span| text.get(..span.start)) else {
    return message.to_string();
  };
  let line = before.matches('\n').count() + 1;
  let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
  format!("line {line}, column {column}: {message}")
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn set_makes_tables_on_the_way() {
    let mut tree = json!({ "a": { "b": 1 }, "c": null });
    set(&mut tree, "a.d", json!(2));
    set(&mut tree, "c.e.f", json!(3));
    assert_eq!(
      tree,
      json!({ "a": { "b": 1, "d": 2 }, "c": { "e": { "f": 3 } } })
    );
    assert_eq!(get(&tree, "c.e.f"), Some(&json!(3)));
    assert_eq!(get(&tree, "a.b.x"), None);
    assert_eq!(get(&tree, "nope"), None);
  }

  #[test]
  fn coerce_reads_each_kind() {
    let list = Kind::List(Box::new(Kind::Integer));
    let cases = [
      ("TRUE", Kind::Bool, Some(json!(true))),
      ("False", Kind::Bool, Some(json!(false))),
      ("yes", Kind::Bool, None),
      ("-12", Kind::Integer, Some(json!(-12))),
      ("18446744073709551615", Kind::Integer, Some(json!(u64::MAX))),
      (" 12", Kind::Integer, None),
      ("1.5", Kind::Float, Some(json!(1.5))),
      ("NaN", Kind::Float, None),
      ("1024", Kind::Size, Some(json!(1024))),
      ("10 MiB", Kind::Size, Some(json!("10 MiB"))),
      ("15m", Kind::Duration, Some(json!("15m"))),
      ("", Kind::String, Some(json!(""))),
    ];
    for (raw, kind, expected) in cases {
      assert_eq!(
        coerce(raw, &kind, Some(';')).ok(),
        expected,
        "{raw:?} as {kind:?}"
      );
    }
    assert_eq!(coerce("1;2", &list, Some(';')), Ok(json!([1, 2])));
    assert!(coerce("1;x", &list, Some(';')).is_err());
    assert_eq!(coerce("", &list, Some(';')), Ok(json!([])));
    let strings = Kind::List(Box::new(Kind::String));
    assert_eq!(
      coerce(" a ;b;", &strings, Some(';')),
      Ok(json!([" a ", "b", ""])),
      "elements are kept as written"
    );
    assert_eq!(coerce("a;b", &strings, None), Ok(json!("a;b")));
    assert!(coerce("x", &Kind::Map, Some(';')).is_err());
  }

  #[test]
  fn toml_values_convert() {
    let table: toml::Table = "a = 1\nb = [1.5, \"x\"]\nc = 1979-05-27T07:32:00Z\n[d]\ne = true"
      .parse()
      .unwrap();
    assert_eq!(
      from_toml(toml::Value::Table(table)),
      Ok(json!({ "a": 1, "b": [1.5, "x"], "c": "1979-05-27T07:32:00Z", "d": { "e": true } }))
    );
    assert!(from_toml(toml::Value::Float(f64::INFINITY)).is_err());
  }
}
