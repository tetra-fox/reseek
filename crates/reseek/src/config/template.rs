//! The commented-out configuration file written on first start: every option with its help and
//! its default, grouped by table, so the file documents itself.

use cascade::{Field, Kind, Schema};
use serde_json::Value;

/// The template's text.
pub fn render(schema: &Schema) -> String {
  let mut text = String::from(
    "# reseek configuration.\n#\n# Every option is listed below with its default, commented out. \
     Uncomment a line and\n# change its value to set it. Files in config.d/ next to this one \
     are read after it, in\n# name order, and win over it. Environment variables and \
     command-line flags are listed by\n# `reseek --envars` and `reseek --help`.\n\n",
  );
  // keys outside every table must come before the first table header
  let (top, nested): (Vec<&Field>, Vec<&Field>) = schema
    .fields()
    .iter()
    .partition(|f| !f.key.contains('.') && f.kind != Kind::Map);
  let mut table = String::new();
  for field in top.into_iter().chain(nested) {
    let (section, name) = match field.key.rsplit_once('.') {
      Some((section, name)) => (section, name),
      None => ("", field.key.as_str()),
    };
    if field.kind == Kind::Map {
      text.push('\n');
      doc(&mut text, field);
      text.push_str(&format!("# [{}.<name>]\n", field.key));
      for entry in &field.entries {
        text.push_str(&format!(
          "# {} = {}\n",
          entry.key,
          toml_value(&entry.default)
        ));
      }
      table.clear();
      continue;
    }
    if section != table && !section.is_empty() {
      text.push_str(&format!("\n# [{section}]\n"));
      table = section.to_string();
    }
    doc(&mut text, field);
    text.push_str(&format!("# {name} = {}\n", toml_value(&field.default)));
  }
  text
}

fn doc(text: &mut String, field: &Field) {
  for line in field.doc.lines().filter(|l| !l.trim().is_empty()) {
    text.push_str(&format!("# {}\n", line.trim()));
  }
}

/// A default as TOML; a key with no default is left without a value to fill in.
fn toml_value(value: &Value) -> String {
  match value {
    Value::Null => String::new(),
    Value::Object(_) => "{}".to_string(),
    other => other.to_string(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::Options;

  #[test]
  fn the_template_is_all_comments_and_lists_each_option_under_its_table() {
    let text = render(&Schema::of::<Options>());
    assert!(text.parse::<toml::Table>().unwrap().is_empty());
    let web = text.find("# [web]\n").unwrap();
    let port = text.find("# port = 5030\n").unwrap();
    assert!(web < port);
    assert!(text.contains("# [web.authentication.api_keys.<name>]\n"));
    assert!(text.contains("# debug = false\n"));
  }
}
