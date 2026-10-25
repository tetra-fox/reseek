//! The banner and the generated lists of command-line flags and environment variables.

use super::HASH_PASSWORD;
use cascade::{Field, Kind, Schema};
use serde_json::Value;

use super::{BOOTSTRAP, CONFIG_FILE};
use crate::version;

/// The startup banner: the name, the version, the licence and the build's markers.
pub fn banner() -> String {
  let info = version::info();
  let mut text = format!(
    "\nreseek {}\nA Soulseek client daemon. AGPL-3.0-only (SPDX-License-Identifier: \
     AGPL-3.0-only)\n",
    info.full
  );
  if info.is_development {
    text.push_str("  development build\n");
  }
  if info.is_canary {
    text.push_str("  canary build\n");
  }
  text
}

/// Every command-line flag, the command line's own first, then the options' in the order the
/// configuration declares them.
pub fn help(schema: &Schema) -> String {
  let mut rows: Vec<(String, String)> = BOOTSTRAP
    .iter()
    .map(|(_, short, long, value, doc)| {
      let mut item = flag_item(*short, long);
      if let Some(value) = value {
        item.push_str(&format!(" <{value}>"));
      }
      (item, doc.to_string())
    })
    .collect();
  rows.push((
    format!("    {CONFIG_FILE} <path>"),
    "The same as --config.".to_string(),
  ));
  for field in schema.fields() {
    let Some(long) = &field.cli else {
      continue;
    };
    let mut item = flag_item(field.short, long);
    if field.kind != Kind::Bool {
      item.push_str(&format!(" <{}>", kind_name(&field.kind)));
    }
    rows.push((item, described(field)));
  }
  format!(
    "\nusage: reseek [arguments]\n       reseek {HASH_PASSWORD}   prints an argon2id hash of a password read from \
     the terminal, or from stdin, for password_hash\n\narguments:\n{}",
    table(&rows)
  )
}

/// Every environment variable an option reads.
pub fn envars(schema: &Schema) -> String {
  let rows: Vec<(String, String)> = schema
    .fields()
    .iter()
    .filter_map(|field| {
      let env = field.env.as_ref()?;
      Some((
        format!("  {env} <{}>", kind_name(&field.kind)),
        described(field),
      ))
    })
    .collect();
  format!(
    "\nenvironment variables (arguments and config file have precedence):\n{}",
    table(&rows)
  )
}

fn flag_item(short: Option<char>, long: &str) -> String {
  match short {
    Some(c) => format!("  -{c}|{long}"),
    None => format!("    {long}"),
  }
}

/// The option's help on one line, with its default unless it is a boolean.
fn described(field: &Field) -> String {
  let doc = field.doc.split_whitespace().collect::<Vec<_>>().join(" ");
  if field.kind == Kind::Bool {
    return doc;
  }
  let default = match &field.default {
    Value::Null => "<null>".to_string(),
    Value::String(s) => s.clone(),
    other => other.to_string(),
  };
  format!("{doc} (default: {default})")
}

fn kind_name(kind: &Kind) -> String {
  match kind {
    Kind::Bool => "bool".to_string(),
    Kind::Integer => "int".to_string(),
    Kind::Float => "float".to_string(),
    Kind::String => "string".to_string(),
    Kind::Duration => "duration".to_string(),
    Kind::Size => "size".to_string(),
    Kind::List(item) => format!("{}[]", kind_name(item)),
    Kind::Map | Kind::Table => "table".to_string(),
  }
}

/// The rows with their items padded to the longest, three spaces, then the help.
fn table(rows: &[(String, String)]) -> String {
  let width = rows.iter().map(|(item, _)| item.len()).max().unwrap_or(0);
  rows
    .iter()
    .map(|(item, doc)| format!("{item:width$}   {doc}\n"))
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::Options;

  #[test]
  fn help_lines_show_the_short_form_type_and_default() {
    let text = help(&Schema::of::<Options>());
    let port = text.lines().find(|l| l.contains("--http-port")).unwrap();
    assert!(port.starts_with("  -l|--http-port <int> "), "{port}");
    assert!(port.ends_with("(default: 5030)"), "{port}");
    let debug = text.lines().find(|l| l.contains("--debug")).unwrap();
    assert!(!debug.contains("default"), "{debug}");
    let socket = text.lines().find(|l| l.contains("--http-socket")).unwrap();
    assert!(socket.starts_with("    --http-socket <string>"), "{socket}");
    assert!(socket.ends_with("(default: <null>)"), "{socket}");
    assert!(text.find("--version").unwrap() < text.find("--debug").unwrap());
  }
}
