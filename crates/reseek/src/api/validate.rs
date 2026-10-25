//! The checks requests share, each written once: limits, durations, sizes of free text, the
//! base64 paths of the files routes and room names. A domain's own checks are in its handler
//! module.

use std::time::Duration;

use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use super::error::ApiError;
use crate::safety::{self, Os};

/// Refuses a page size outside 1 to `max`.
pub fn limit(name: &str, value: u32, max: u32) -> Result<(), ApiError> {
  if value == 0 {
    return Err(ApiError::bad_request(format!(
      "{name} must be greater than zero"
    )));
  }
  if value > max {
    return Err(ApiError::bad_request(format!(
      "{name} must be at most {max}"
    )));
  }
  Ok(())
}

/// Refuses a page size of zero.
pub fn positive(name: &str, value: u32) -> Result<(), ApiError> {
  limit(name, value, u32::MAX)
}

/// A duration written as `"15s"`, `"2h 30m"` or `"7d"`.
pub fn duration(name: &str, text: &str) -> Result<Duration, ApiError> {
  humantime::parse_duration(text.trim())
    .map_err(|e| ApiError::invalid(format!("{name} is not a duration"), e.to_string()))
}

/// The path a files route names in base64 of its UTF-8 text, standard or URL-safe, padded or
/// not, as a `/`-separated path relative to its root. `\` counts as a separator; a deletion
/// drops leading separators, which older clients send. An absolute path, a `.` or `..`
/// segment and a NUL are refused.
pub fn files_path(encoded: &str, deleting: bool) -> Result<String, ApiError> {
  let config = GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent);
  let engine = if encoded.contains(['+', '/']) {
    GeneralPurpose::new(&alphabet::STANDARD, config)
  } else {
    GeneralPurpose::new(&alphabet::URL_SAFE, config)
  };
  let bytes = engine
    .decode(encoded)
    .map_err(|e| ApiError::invalid("the path is not base64", e.to_string()))?;
  let text =
    String::from_utf8(bytes).map_err(|_| ApiError::bad_request("the path is not UTF-8"))?;
  if text.contains('\0') {
    return Err(ApiError::bad_request("the path holds a NUL character"));
  }
  let mut path = text.replace('\\', "/");
  if deleting {
    path = path.trim_start_matches('/').to_string();
  }
  if safety::is_absolute(&path, Os::Unix) || safety::is_absolute(&path, Os::Windows) {
    return Err(ApiError::bad_request(
      "Only relative paths may be specified",
    ));
  }
  if safety::traverses(&path) {
    return Err(ApiError::bad_request(
      "Paths containing traversal segments are not allowed",
    ));
  }
  Ok(path)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn files_paths_decode_either_alphabet_and_refuse_escapes() {
    let encode = |text: &str| base64::engine::general_purpose::STANDARD.encode(text);
    let url_safe = |text: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(text);
    assert_eq!(
      files_path(&encode("Album/01.flac"), false).unwrap(),
      "Album/01.flac"
    );
    // one name, two encodings: the standard one holds `/` and `+`, the URL-safe one `_` and `-`
    let odd = "Caf\u{e9} ?>~/x";
    assert!(encode(odd).contains(['/', '+']), "{}", encode(odd));
    assert_eq!(files_path(&encode(odd), false).unwrap(), odd);
    assert_eq!(files_path(&url_safe(odd), false).unwrap(), odd);
    assert_eq!(files_path(&encode("a\\b"), false).unwrap(), "a/b");
    assert_eq!(files_path("", false).unwrap(), "", "the root itself");
    assert_eq!(files_path(&encode("/name"), true).unwrap(), "name");
    for bad in ["/etc", "../x", "a/./b", "C:\\x", "\\\\server\\x", "a\0b"] {
      assert_eq!(
        files_path(&encode(bad), false).unwrap_err().status(),
        400,
        "{bad:?}"
      );
    }
    assert_eq!(files_path("!!!", false).unwrap_err().status(), 400);
    assert_eq!(
      files_path("/w==", false).unwrap_err().status(),
      400,
      "not UTF-8"
    );
  }

  #[test]
  fn limits_take_one_to_the_maximum() {
    assert!(limit("limit", 1, 10).is_ok());
    assert!(limit("limit", 10, 10).is_ok());
    assert!(limit("limit", 0, 10).is_err());
    assert!(limit("limit", 11, 10).is_err());
  }
}
