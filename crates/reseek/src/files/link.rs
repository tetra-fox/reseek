//! The tokens that let a link load one file without a session: a JWT signed with HS256 under
//! the daemon's file-link key, naming the principal it was made for, the one file it opens and
//! when it stops being accepted, ten minutes after it was made.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::secret::Secret;

/// How long a token is accepted.
pub(super) const LIFETIME: Duration = Duration::minutes(10);

/// The header every token carries.
const HEADER: &str = r#"{"alg":"HS256","typ":"JWT"}"#;

/// What a token says.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Claims {
  /// the subject of the principal it was made for
  sub: String,
  /// the file it opens: its root and its path under it, such as `downloads/Album/01.flac`
  path: String,
  /// when it stops being accepted, in seconds since the epoch
  exp: i64,
  /// its own id
  jti: String,
}

/// The name a token gives the file `path` under the root named `root`.
pub(super) fn claimed(root: &str, path: &str) -> String {
  format!("{root}/{path}")
}

/// A token for the file `path`, as [`claimed`] names it, made for `subject` at `now`.
pub(super) fn issue(
  secret: &Secret,
  subject: &str,
  path: &str,
  now: DateTime<Utc>,
) -> (String, DateTime<Utc>) {
  let expires_at = now + LIFETIME;
  let claims = Claims {
    sub: subject.to_string(),
    path: path.to_string(),
    exp: expires_at.timestamp(),
    jti: Uuid::new_v4().to_string(),
  };
  let claims = serde_json::to_string(&claims).expect("the claims are plain JSON");
  let signed = format!(
    "{}.{}",
    URL_SAFE_NO_PAD.encode(HEADER),
    URL_SAFE_NO_PAD.encode(claims)
  );
  let signature = secret.sign_link(&signed);
  (format!("{signed}.{signature}"), expires_at)
}

/// The subject `token` was made for, when it is signed by this daemon, unexpired at `now` and
/// opens `path`; else why not.
pub(super) fn check(
  secret: &Secret,
  token: &str,
  path: &str,
  now: DateTime<Utc>,
) -> Result<String, &'static str> {
  let mut parts = token.split('.');
  let (Some(header), Some(claims), Some(signature), None) =
    (parts.next(), parts.next(), parts.next(), parts.next())
  else {
    return Err("not a token");
  };
  if !secret.verify_link(&format!("{header}.{claims}"), signature) {
    return Err("not signed by this daemon");
  }
  let header = URL_SAFE_NO_PAD
    .decode(header)
    .map_err(|_| "a header that does not read")?;
  let header: serde_json::Value =
    serde_json::from_slice(&header).map_err(|_| "a header that does not read")?;
  if header["alg"] != "HS256" {
    return Err("not signed with HS256");
  }
  let claims = URL_SAFE_NO_PAD
    .decode(claims)
    .map_err(|_| "claims that do not read")?;
  let claims: Claims = serde_json::from_slice(&claims).map_err(|_| "claims that do not read")?;
  if claims.exp <= now.timestamp() {
    return Err("expired");
  }
  if claims.path != path {
    return Err("made for another file");
  }
  Ok(claims.sub)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn secret(master: &str) -> Secret {
    Secret::for_tests(master.as_bytes())
  }

  #[test]
  fn a_token_opens_its_one_file_until_it_expires() {
    let key = secret("one");
    let now: DateTime<Utc> = "2026-10-25T12:00:00Z".parse().unwrap();
    let file = claimed("downloads", "Album/01.flac");
    let (token, expires_at) = issue(&key, "local:admin", &file, now);
    assert_eq!(expires_at, now + Duration::minutes(10));
    assert_eq!(check(&key, &token, &file, now).unwrap(), "local:admin");
    assert_eq!(
      check(&key, &token, &claimed("downloads", "Album/02.flac"), now),
      Err("made for another file")
    );
    assert_eq!(
      check(&key, &token, &claimed("incomplete", "Album/01.flac"), now),
      Err("made for another file")
    );
    assert_eq!(check(&key, &token, &file, expires_at), Err("expired"));
    assert_eq!(
      check(&secret("other"), &token, &file, now),
      Err("not signed by this daemon")
    );
    let mut tampered = token.clone();
    tampered.insert(token.find('.').unwrap() + 2, 'x');
    assert!(check(&key, &tampered, &file, now).is_err());
    assert_eq!(check(&key, "abc", &file, now), Err("not a token"));
  }
}
