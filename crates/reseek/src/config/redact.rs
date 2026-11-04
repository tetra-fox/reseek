//! Showing configuration values without their secrets: what every API response, log line and
//! debug view of the options goes through, and putting back the URL credentials a client sends
//! back hidden.

use std::collections::BTreeMap;

use cascade::{Invalid, Loaded, REDACTED};
use serde_json::Value;

use super::Options;

/// The values of `loaded` with every secret shown as `*****` and the credentials taken out of
/// every URL.
pub fn values(loaded: &Loaded<Options>) -> Value {
  let mut values = loaded.redacted();
  urls(&mut values);
  values
}

/// `value` with the credentials taken out of every URL in it, at any depth. A URL's
/// `user:password@` becomes `*****@`.
pub fn urls(value: &mut Value) {
  match value {
    Value::String(text) => {
      if let Some(hidden) = without_credentials(text) {
        *text = hidden;
      }
    }
    Value::Array(items) => items.iter_mut().for_each(urls),
    Value::Object(entries) => entries.values_mut().for_each(urls),
    _ => {}
  }
}

/// Puts back the credentials of every URL in `set`, new values by key, that shows them as
/// `*****@`: what a client read had them taken out, and it sends a list or a map back whole when
/// it changes one entry. Such a URL takes the URL under the same key in `current`, the values in
/// effect, that shows as the same text. One that matches none, or URLs that differ, is refused
/// with where it is, and the text `*****@` never takes the place of credentials.
pub fn restore_urls(set: &mut BTreeMap<String, Value>, current: &Value) -> Result<(), Invalid> {
  for (key, value) in set.iter_mut() {
    let mut held = Vec::new();
    if let Some(now) = key
      .split('.')
      .try_fold(current, |node, part| node.get(part))
    {
      credentialed(now, &mut held);
    }
    restore(value, key, &held).map_err(|path| {
      Invalid::new(
        key.clone(),
        format!(
          "The URL at {path} has its credentials hidden and none to keep here. Enter it again."
        ),
      )
    })?;
  }
  Ok(())
}

/// Every URL with credentials in `value`, at any depth, as it is shown and as it is.
fn credentialed<'a>(value: &'a Value, found: &mut Vec<(String, &'a str)>) {
  match value {
    Value::String(text) => found.extend(without_credentials(text).map(|shown| (shown, &**text))),
    Value::Array(items) => items.iter().for_each(|item| credentialed(item, found)),
    Value::Object(entries) => entries
      .values()
      .for_each(|entry| credentialed(entry, found)),
    _ => {}
  }
}

/// [`restore_urls`] for `value` at `path`, whose error is the path of a URL it cannot restore.
fn restore(value: &mut Value, path: &str, held: &[(String, &str)]) -> Result<(), String> {
  match value {
    Value::String(text) if hides_credentials(text) => {
      let mut matching = held
        .iter()
        .filter(|(shown, _)| shown == text)
        .map(|(_, real)| *real);
      let real = matching.next().ok_or_else(|| path.to_string())?;
      if matching.any(|other| other != real) {
        return Err(path.to_string());
      }
      *text = real.to_string();
    }
    Value::Array(items) => {
      for (index, item) in items.iter_mut().enumerate() {
        restore(item, &format!("{path}[{index}]"), held)?;
      }
    }
    Value::Object(entries) => {
      for (name, entry) in entries {
        restore(entry, &format!("{path}.{name}"), held)?;
      }
    }
    _ => {}
  }
  Ok(())
}

/// Whether `text` is a URL whose credentials are shown as `*****@`.
fn hides_credentials(text: &str) -> bool {
  url::Url::parse(text).is_ok_and(|url| url.username() == REDACTED && url.password().is_none())
}

/// `text` with its credentials replaced when it is a URL that has some, else as it is.
pub fn url(text: &str) -> String {
  without_credentials(text).unwrap_or_else(|| text.to_string())
}

/// `text` with its credentials replaced, when it is a URL that has some.
fn without_credentials(text: &str) -> Option<String> {
  let mut url = url::Url::parse(text).ok()?;
  if url.username().is_empty() && url.password().is_none() {
    return None;
  }
  url.set_password(None).ok()?;
  url.set_username(REDACTED).ok()?;
  Some(url.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn credentials_are_taken_out_of_urls_anywhere() {
    let mut value = json!({
      "relay": { "controller": { "address": "https://agent:hunter2@controller.lan:5031/" } },
      "logger": { "loki": "http://loki:3100" },
      "rooms": ["https://user@example.net/x"],
      "soulseek": { "description": "not a url: a:b@c" },
    });
    urls(&mut value);
    assert_eq!(
      value["relay"]["controller"]["address"],
      "https://*****@controller.lan:5031/"
    );
    assert_eq!(value["logger"]["loki"], "http://loki:3100");
    assert_eq!(value["rooms"][0], "https://*****@example.net/x");
    assert_eq!(value["soulseek"]["description"], "not a url: a:b@c");
  }

  #[test]
  fn a_hidden_url_takes_the_one_it_shows_and_one_it_cannot_tell_is_refused() {
    let current = json!({ "hooks": { "urls": [
      "https://u:p@x.lan/", "https://v:q@y.lan/", "https://w:r@y.lan/", "http://plain.lan/"
    ] } });
    let mut set = BTreeMap::from([(
      "hooks.urls".to_string(),
      json!(["new", "https://*****@x.lan/", "http://plain.lan/"]),
    )]);
    restore_urls(&mut set, &current).unwrap();
    assert_eq!(
      set["hooks.urls"],
      json!(["new", "https://u:p@x.lan/", "http://plain.lan/"])
    );
    // two URLs with different credentials show as the same text
    let mut either = BTreeMap::from([("hooks.urls".to_string(), json!(["https://*****@y.lan/"]))]);
    let refused = restore_urls(&mut either, &current).unwrap_err();
    assert_eq!(refused.key, "hooks.urls");
    assert!(
      refused.message.contains("hooks.urls[0]"),
      "{}",
      refused.message
    );
    let mut nothing = BTreeMap::from([("other".to_string(), json!("https://*****@x.lan/"))]);
    assert!(restore_urls(&mut nothing, &current).is_err());
  }
}
