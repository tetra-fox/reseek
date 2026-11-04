//! Checks many options share: ranges, lengths, required values, durations and URLs. Each
//! returns the message for a value that fails, naming the option by `name`, for an
//! [`Invalid`](crate::Invalid) under its key.

use std::fmt::Display;
use std::time::Duration;

/// `value` is from `min` to `max`, inclusive.
pub fn range<T: PartialOrd + Display>(name: &str, value: T, min: T, max: T) -> Result<(), String> {
  if value < min || value > max {
    return Err(format!("The field {name} must be between {min} and {max}."));
  }
  Ok(())
}

/// `value` is at least `min`; there is no upper limit.
pub fn at_least<T: PartialOrd + Display>(name: &str, value: T, min: T) -> Result<(), String> {
  if value < min {
    return Err(format!("The field {name} must be at least {min}."));
  }
  Ok(())
}

/// `value`, when set, is at least `min`.
pub fn optional_at_least<T: PartialOrd + Display + Copy>(
  name: &str,
  value: Option<T>,
  min: T,
) -> Result<(), String> {
  value.map_or(Ok(()), |v| at_least(name, v, min))
}

/// `value` is at least `min`.
pub fn duration_at_least(name: &str, value: Duration, min: Duration) -> Result<(), String> {
  if value < min {
    return Err(format!(
      "The field {name} must be at least {}.",
      crate::duration::written(min)
    ));
  }
  Ok(())
}

/// `value` is `0`, which turns its limit off, or at least `min`.
pub fn duration_off_or_at_least(name: &str, value: Duration, min: Duration) -> Result<(), String> {
  if !value.is_zero() && value < min {
    return Err(format!(
      "The field {name} must be 0 (disabled) or at least {}.",
      crate::duration::written(min)
    ));
  }
  Ok(())
}

/// `value` is `0`, which turns its limit off, or at least `min`.
pub fn off_or_at_least(name: &str, value: u32, min: u32) -> Result<(), String> {
  if value != 0 && value < min {
    return Err(format!(
      "The field {name} must be 0 (disabled) or at least {min}."
    ));
  }
  Ok(())
}

/// A string, when set, is from `min` to `max` characters long.
pub fn length(name: &str, value: Option<&str>, min: usize, max: usize) -> Result<(), String> {
  let Some(value) = value else {
    return Ok(());
  };
  let length = value.chars().count();
  if length < min || length > max {
    return Err(format!(
      "The field {name} must be a string with a minimum length of {min} and a maximum length of \
       {max}."
    ));
  }
  Ok(())
}

/// A string is set and holds more than whitespace.
pub fn required(name: &str, value: Option<&str>) -> Result<(), String> {
  match value {
    None | Some("") => Err(format!("The field {name} must contain a value.")),
    Some(v) if v.trim().is_empty() => Err(format!(
      "The field {name} must not contain only whitespace."
    )),
    Some(_) => Ok(()),
  }
}

/// An absolute `http` or `https` URL with a host.
pub fn http_url(name: &str, value: &str) -> Result<(), String> {
  match url::Url::parse(value) {
    Ok(url) if matches!(url.scheme(), "http" | "https") && url.has_host() => Ok(()),
    _ => Err(format!(
      "The field {name} must be an absolute http or https URL."
    )),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn urls_are_absolute_http() {
    assert!(http_url("x", "https://user:pw@host:5031/base").is_ok());
    assert!(http_url("x", "http://localhost:3100").is_ok());
    for bad in ["ftp://host", "host:5031", "/relative", "https://", ""] {
      assert!(http_url("x", bad).is_err(), "{bad}");
    }
  }

  #[test]
  fn required_strings_need_more_than_whitespace() {
    assert_eq!(
      required("x", None).unwrap_err(),
      "The field x must contain a value."
    );
    assert_eq!(
      required("x", Some(" ")).unwrap_err(),
      "The field x must not contain only whitespace."
    );
    assert!(required("x", Some("a")).is_ok());
  }
}
