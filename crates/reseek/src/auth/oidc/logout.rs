//! Logging out with an OpenID Connect provider: the provider ending our sessions through a
//! signed logout token it posts to us, and our logout sending the browser on to the provider's
//! own logout page.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use openidconnect::JsonWebKey;
use openidconnect::core::CoreJwsSigningAlgorithm;
use serde_json::Value;
use url::Url;

use super::flow::payload;
use super::{Metadata, Provider};

/// The event a logout token carries.
const BACKCHANNEL_EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";

/// How far the provider's clock may be from ours, in seconds.
const LEEWAY: i64 = 300;

/// Who a valid logout token signs out: a provider session, or else a subject.
#[derive(Debug, PartialEq, Eq)]
pub struct LoggedOut {
  /// the provider's session id
  pub sid: Option<String>,
  /// the provider's subject
  pub sub: Option<String>,
}

/// Checks a logout token like an ID token, without a nonce and with the back-channel logout
/// event, and says who it signs out. The provider's keys are read again once when the token
/// names a key not seen yet.
pub async fn check_token(
  provider: &Provider,
  metadata: Metadata,
  token: &str,
) -> Result<LoggedOut, String> {
  let signed = match signed(&metadata, token) {
    Err(Unverified::NoKey) => match provider.refresh_keys().await {
      Some(fresh) => signed(&fresh, token),
      None => Err(Unverified::NoKey),
    },
    other => other,
  };
  signed.map_err(|e| e.to_string())?;
  let claims = payload(token).ok_or_else(|| "the claims do not read".to_string())?;
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
  match claims.get("iat").and_then(Value::as_i64) {
    Some(issued) if issued <= now + LEEWAY => {}
    Some(_) => return Err("issued in the future".to_string()),
    None => return Err("no issue time".to_string()),
  }
  if claims
    .get("exp")
    .and_then(Value::as_i64)
    .is_some_and(|expires| expires + LEEWAY < now)
  {
    return Err("expired".to_string());
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

/// Checks the signature of the compact JWT `token` with the keys in `metadata`.
fn signed(metadata: &Metadata, token: &str) -> Result<(), Unverified> {
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
  if verified {
    Ok(())
  } else {
    Err(Unverified::NoKey)
  }
}

/// The provider's logout page for a session that holds `id_token`, returning the browser to
/// `back` afterwards; `None` when the provider has none.
pub fn end_session_url(metadata: &Metadata, id_token: &str, back: &str) -> Option<String> {
  let endpoint = metadata
    .additional_metadata()
    .end_session_endpoint
    .as_ref()?;
  let mut url: Url = endpoint.url().clone();
  url
    .query_pairs_mut()
    .append_pair("id_token_hint", id_token)
    .append_pair("post_logout_redirect_uri", back);
  Some(url.to_string())
}
