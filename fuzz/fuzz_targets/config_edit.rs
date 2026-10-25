#![no_main]

use std::collections::BTreeMap;

use cascade::{FileEditor, Schema};
use libfuzzer_sys::fuzz_target;
use reseek_fuzz::config;

// a file's text, a byte picking a key, and a JSON value for it: an edit of text that parses
// either fails with a reason or leaves text that still parses
fuzz_target!(|data: &[u8]| {
  let Some(split) = data.iter().position(|&b| b == 0) else {
    return;
  };
  let (text, rest) = data.split_at(split);
  let Some((&pick, value)) = rest[1..].split_first() else {
    return;
  };
  let (Ok(text), Ok(value)) = (
    std::str::from_utf8(text),
    serde_json::from_slice::<serde_json::Value>(value),
  ) else {
    return;
  };
  if text.parse::<toml_edit::DocumentMut>().is_err() {
    return;
  }
  let schema = Schema::of::<config::Options>();
  let key = schema.fields()[pick as usize % schema.fields().len()]
    .key
    .clone();
  let editor = FileEditor::new(&schema);
  let (set, unset) = if pick >= 128 {
    (BTreeMap::new(), vec![key])
  } else {
    (BTreeMap::from([(key, value)]), Vec::new())
  };
  if let Ok(edited) = editor.patch("config.toml", text, &set, &unset) {
    assert!(
      edited.parse::<toml_edit::DocumentMut>().is_ok(),
      "the edit left text that does not parse:\n{edited}"
    );
  }
});
