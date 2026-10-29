//! A conversation or a room's history as a file to save: a JSON array of its messages, or text
//! with one message per line, a message's own line breaks indented under it.

use api_types::messaging::ExportFormat;
use api_types::messaging::conversations::{MessageDirection, PrivateMessage};
use api_types::messaging::rooms::RoomMessage;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;

use crate::api::error::ApiError;

/// One line of the text form: when, who, and what, the message's line breaks indented.
fn line(at: DateTime<Utc>, author: &str, text: &str) -> String {
  let text = text.replace('\n', "\n  ");
  format!(
    "{} {author}: {text}\n",
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
  )
}

/// `name` as it can stand in a file name.
fn file_safe(name: &str) -> String {
  name
    .chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
        c
      } else {
        '_'
      }
    })
    .collect()
}

/// The response that saves `body` as `<kind>-<name>-<date>.<extension>`.
fn attachment(kind: &str, name: &str, format: ExportFormat, body: Vec<u8>) -> Response {
  let (extension, content_type) = match format {
    ExportFormat::Json => ("json", "application/json"),
    ExportFormat::Text => ("txt", "text/plain; charset=utf-8"),
  };
  let file = format!(
    "{kind}-{}-{}.{extension}",
    file_safe(name),
    Utc::now().format("%Y-%m-%d")
  );
  let mut response = body.into_response();
  let headers = response.headers_mut();
  headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
  let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{file}\""))
    .expect("a file name of ASCII letters, digits and -_. is a header value");
  headers.insert(header::CONTENT_DISPOSITION, disposition);
  response
}

fn json(messages: &impl Serialize) -> Result<Vec<u8>, ApiError> {
  serde_json::to_vec(messages).map_err(ApiError::internal)
}

/// The conversation with `username` as a file; `us` is the author of the messages we sent.
pub fn conversation(
  username: &str,
  us: &str,
  messages: &[PrivateMessage],
  format: ExportFormat,
) -> Result<Response, ApiError> {
  let body = match format {
    ExportFormat::Json => json(&messages)?,
    ExportFormat::Text => messages
      .iter()
      .map(|m| {
        let author = match m.direction {
          MessageDirection::In => &m.username,
          MessageDirection::Out => us,
        };
        line(m.timestamp, author, &m.message)
      })
      .collect::<String>()
      .into_bytes(),
  };
  Ok(attachment("chat", username, format, body))
}

/// The stored history of `room` as a file.
pub fn room(
  room: &str,
  messages: &[RoomMessage],
  format: ExportFormat,
) -> Result<Response, ApiError> {
  let body = match format {
    ExportFormat::Json => json(&messages)?,
    ExportFormat::Text => messages
      .iter()
      .map(|m| line(m.timestamp, &m.username, &m.message))
      .collect::<String>()
      .into_bytes(),
  };
  Ok(attachment("room", room, format, body))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_text_line_keeps_a_message_on_its_entry() {
    let at = "2026-10-26T10:00:00.5Z".parse().unwrap();
    assert_eq!(
      line(at, "alice", "hi\nthere"),
      "2026-10-26T10:00:00Z alice: hi\n  there\n"
    );
    assert_eq!(file_safe("a b/../c"), "a_b_.._c");
  }
}
