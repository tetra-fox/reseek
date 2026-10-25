//! Reading cookies from a request and writing the ones the daemon sets: always `HttpOnly` and
//! `SameSite=Lax`, `Secure` when the client spoke HTTPS, scoped to the URL base.

use std::time::Duration;

use axum::http::{HeaderMap, HeaderValue, header};

/// The session cookie's name.
pub const SESSION: &str = "reseek_session";

/// The value of the cookie `name` in `headers`.
pub fn find<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
  headers
    .get_all(header::COOKIE)
    .iter()
    .filter_map(|value| value.to_str().ok())
    .flat_map(|line| line.split(';'))
    .filter_map(|pair| pair.trim().split_once('='))
    .find(|(key, _)| *key == name)
    .map(|(_, value)| value.trim())
}

/// Where a cookie applies and whether it needs HTTPS.
#[derive(Clone, Debug)]
pub struct Scope {
  /// the URL base, `/` at the root
  pub path: String,
  /// the client spoke HTTPS
  pub secure: bool,
}

impl Scope {
  /// The scope for a daemon under `url_base` (empty at the root) and a client that did or did
  /// not speak HTTPS.
  pub fn new(url_base: &str, secure: bool) -> Self {
    Self {
      path: if url_base.is_empty() {
        "/".to_string()
      } else {
        url_base.to_string()
      },
      secure,
    }
  }

  /// A `Set-Cookie` value setting `name` to `value` for `max_age`.
  pub fn set(&self, name: &str, value: &str, max_age: Duration) -> HeaderValue {
    self.header(name, value, max_age.as_secs())
  }

  /// A `Set-Cookie` value removing `name`.
  pub fn clear(&self, name: &str) -> HeaderValue {
    self.header(name, "", 0)
  }

  fn header(&self, name: &str, value: &str, max_age: u64) -> HeaderValue {
    let secure = if self.secure { "; Secure" } else { "" };
    let text = format!(
      "{name}={value}; Path={}; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}",
      self.path
    );
    HeaderValue::from_str(&text).expect("cookie names, values and paths are header-safe text")
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn cookies_are_found_among_others_and_written_scoped() {
    let mut headers = HeaderMap::new();
    headers.append(
      header::COOKIE,
      "theme=dark; reseek_session=abc.def".parse().unwrap(),
    );
    headers.append(header::COOKIE, "oidc_home=sealed".parse().unwrap());
    assert_eq!(find(&headers, SESSION), Some("abc.def"));
    assert_eq!(find(&headers, "oidc_home"), Some("sealed"));
    assert_eq!(find(&headers, "missing"), None);
    let scope = Scope::new("/reseek", true);
    assert_eq!(
      scope.set(SESSION, "v", Duration::from_secs(60)),
      "reseek_session=v; Path=/reseek; Max-Age=60; HttpOnly; SameSite=Lax; Secure"
    );
    assert_eq!(
      Scope::new("", false).clear(SESSION),
      "reseek_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"
    );
  }
}
