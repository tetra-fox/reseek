//! Log lines shared by the message decoders, so every family reports with the same fields

use crate::error::DecodeError;

/// remote strings are cut to this many bytes in log fields
const MAX_LOGGED_BYTES: usize = 128;

/// The fields worth logging for a decoded message; credentials and contents never go here
#[derive(Default)]
pub(crate) struct KeyFields<'a> {
  pub(crate) username: Option<&'a str>,
  pub(crate) room: Option<&'a str>,
  pub(crate) token: Option<u32>,
  pub(crate) filename: Option<&'a str>,
  pub(crate) count: Option<usize>,
}

fn truncate(text: &str) -> &str {
  if text.len() <= MAX_LOGGED_BYTES {
    return text;
  }
  let mut end = MAX_LOGGED_BYTES;
  while !text.is_char_boundary(end) {
    end -= 1;
  }
  &text[..end]
}

pub(crate) fn decoded(family: &'static str, code: u32, kind: &'static str, fields: KeyFields) {
  tracing::debug!(
    family,
    code,
    kind,
    username = fields.username.map(truncate),
    room = fields.room.map(truncate),
    token = fields.token,
    filename = fields.filename.map(truncate),
    count = fields.count,
    "message decoded"
  );
}

/// An unknown code is expected (new server features appear without notice) and a bad
/// distributed search identifier is a known peer bug, so both are debug; everything else is
/// malformed input and a warning
pub(crate) fn rejected(
  family: &'static str,
  code: u32,
  kind: Option<&'static str>,
  error: &DecodeError,
) {
  match error {
    DecodeError::UnknownCode { .. } => {
      tracing::debug!(family, code, kind, "message with unknown code ignored");
    }
    DecodeError::BadSearchIdentifier { .. } => {
      tracing::debug!(family, code, kind, error = %error, "distributed search rejected");
    }
    _ => tracing::warn!(family, code, kind, error = %error, "malformed message rejected"),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn truncate_respects_char_boundaries() {
    let text = "\u{e9}".repeat(100);
    let cut = truncate(&text);
    assert!(cut.len() <= MAX_LOGGED_BYTES);
    assert!(cut.chars().all(|c| c == '\u{e9}'));
    assert_eq!(truncate("short"), "short");
  }
}
