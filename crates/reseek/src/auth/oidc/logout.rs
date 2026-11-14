//! Logging out with an OpenID Connect provider: the provider ending our sessions through a
//! signed logout token it posts to us, and our logout sending the browser on to the provider's
//! own logout page.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use openidconnect::core::CoreJwsSigningAlgorithm;
use openidconnect::{JsonWebKey, LogoutRequest, PostLogoutRedirectUrl};
use serde_json::Value;
use tracing::warn;

use super::{Keys, LEEWAY, Metadata, OidcIdToken, Provider};

/// The event a logout token carries.
const BACKCHANNEL_EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";

/// Who a valid logout token signs out: a provider session, or else a subject.
#[derive(Debug, PartialEq, Eq)]
pub struct LoggedOut {
  /// the provider's session id
  pub sid: Option<String>,
  /// the provider's subject
  pub sub: Option<String>,
}

/// Why a logout token was not taken.
#[derive(Debug, thiserror::Error)]
pub enum NotTaken {
  /// the token is not a valid logout token of the provider
  #[error("{0}")]
  Invalid(String),
  /// no key in use made its signature, and the provider's keys could not be read again to look
  /// for the one that did
  #[error("the provider's signing keys could not be read to check the token")]
  Unchecked,
}

/// Checks a logout token like an ID token, without a nonce and with the back-channel logout
/// event, and says who it signs out. The provider's keys are read again once when the token
/// names a key not seen yet.
pub async fn check_token(
  provider: &Provider,
  metadata: Metadata,
  token: &str,
) -> Result<LoggedOut, NotTaken> {
  let claims = match signed(&metadata, token) {
    Err(Unverified::NoKey) => match provider.refresh_keys().await {
      Keys::Read(fresh) => signed(&fresh, token),
      Keys::Current => Err(Unverified::NoKey),
      Keys::Unread => return Err(NotTaken::Unchecked),
    },
    other => other,
  }
  .map_err(|e| NotTaken::Invalid(e.to_string()))?;
  logged_out(provider, &metadata, &claims).map_err(NotTaken::Invalid)
}

/// Who the signed claims of a logout token sign out, once they are checked.
fn logged_out(
  provider: &Provider,
  metadata: &Metadata,
  claims: &Value,
) -> Result<LoggedOut, String> {
  let text = |name: &str| claims.get(name).and_then(Value::as_str).map(str::to_string);
  if text("iss").as_deref() != Some(metadata.issuer().as_str()) {
    return Err("issued by another issuer".to_string());
  }
  let client_id = provider.options.client_id.as_deref().unwrap_or_default();
  let audience = match claims.get("aud") {
    Some(Value::String(one)) => one == client_id,
    Some(Value::Array(many)) => many.iter().any(|a| a.as_str() == Some(client_id)),
    _ => false,
  };
  if !audience {
    return Err("meant for another client".to_string());
  }
  let now = Utc::now().timestamp();
  let leeway = LEEWAY.num_seconds();
  match claims.get("iat").and_then(Value::as_i64) {
    Some(issued) if issued <= now + leeway => {}
    Some(_) => return Err("issued in the future".to_string()),
    None => return Err("no issue time".to_string()),
  }
  match claims.get("exp").and_then(Value::as_i64) {
    Some(expires) if expires + leeway >= now => {}
    Some(_) => return Err("expired".to_string()),
    None => return Err("no expiry".to_string()),
  }
  // required but not remembered: a replay before `exp` only ends the same sessions again
  if !claims.get("jti").is_some_and(Value::is_string) {
    return Err("no token id".to_string());
  }
  if claims.get("nonce").is_some() {
    return Err("a logout token has no nonce".to_string());
  }
  if !claims
    .get("events")
    .and_then(|events| events.get(BACKCHANNEL_EVENT))
    .is_some_and(Value::is_object)
  {
    return Err("not a back-channel logout event".to_string());
  }
  let logged_out = LoggedOut {
    sid: text("sid"),
    sub: text("sub"),
  };
  if logged_out.sid.is_none() && logged_out.sub.is_none() {
    return Err("names neither a session nor a subject".to_string());
  }
  Ok(logged_out)
}

/// Why a token's signature was not accepted.
#[derive(Debug, thiserror::Error)]
enum Unverified {
  #[error("the token is not a signed JWT")]
  Malformed,
  #[error("the token is signed with {0}, which is not RS256 or ES256")]
  Algorithm(String),
  #[error("no key of the provider made the signature")]
  NoKey,
}

/// The claims of the compact JWT `token`, once its signature is checked with the keys in
/// `metadata`.
fn signed(metadata: &Metadata, token: &str) -> Result<Value, Unverified> {
  let mut parts = token.split('.');
  let (Some(header_text), Some(body), Some(signature), None) =
    (parts.next(), parts.next(), parts.next(), parts.next())
  else {
    return Err(Unverified::Malformed);
  };
  let header: Value = URL_SAFE_NO_PAD
    .decode(header_text)
    .ok()
    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    .ok_or(Unverified::Malformed)?;
  let signature = URL_SAFE_NO_PAD
    .decode(signature)
    .map_err(|_| Unverified::Malformed)?;
  let alg = header
    .get("alg")
    .and_then(Value::as_str)
    .unwrap_or_default();
  let algorithm = match alg {
    "RS256" => CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
    "ES256" => CoreJwsSigningAlgorithm::EcdsaP256Sha256,
    other => return Err(Unverified::Algorithm(other.to_string())),
  };
  let kid = header.get("kid").and_then(Value::as_str);
  let message = format!("{header_text}.{body}");
  let verified = metadata
    .jwks()
    .keys()
    .iter()
    .filter(|key| kid.is_none() || key.key_id().map(|id| id.as_str()) == kid)
    .any(|key| {
      key
        .verify_signature(&algorithm, message.as_bytes(), &signature)
        .is_ok()
    });
  if !verified {
    return Err(Unverified::NoKey);
  }
  URL_SAFE_NO_PAD
    .decode(body)
    .ok()
    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    .ok_or(Unverified::Malformed)
}

/// The provider's logout page for a session that holds `id_token`, returning the browser to
/// `back` afterwards; `None` when the provider has none.
pub fn end_session_url(metadata: &Metadata, id_token: &str, back: &str) -> Option<String> {
  let endpoint = metadata
    .additional_metadata()
    .end_session_endpoint
    .clone()?;
  let mut request = LogoutRequest::from(endpoint);
  match id_token.parse::<OidcIdToken>() {
    Ok(token) => request = request.set_id_token_hint(&token),
    Err(e) => warn!(error = %e, "a stored ID token does not read; logging out without it"),
  }
  match PostLogoutRedirectUrl::new(back.to_string()) {
    Ok(url) => request = request.set_post_logout_redirect_uri(url),
    Err(e) => warn!(error = %e, "the address to return to after logout is not a URL"),
  }
  Some(request.http_get_url().to_string())
}
