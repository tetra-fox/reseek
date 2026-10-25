//! Showing configuration values without their secrets: what every API response, log line and
//! debug view of the options goes through.

use cascade::{Loaded, REDACTED};
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
}
