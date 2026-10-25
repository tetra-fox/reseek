//! A stand-in OpenID Connect provider on a local port: discovery, signing keys, the token
//! endpoint and userinfo. The test plays the browser at the authorization endpoint: it reads
//! the login's parameters from the redirect and grants a code with the claims it wants.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use openidconnect::core::{CoreJwsSigningAlgorithm, CoreRsaPrivateSigningKey};
use openidconnect::{JsonWebKeyId, PrivateSigningKey};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::pkcs8::DecodePrivateKey;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The client id the provider knows the daemon by.
pub const CLIENT_ID: &str = "reseek";
/// The client secret the daemon authenticates to the token endpoint with.
pub const CLIENT_SECRET: &str = "s3cret";

/// What a code was granted for: the PKCE challenge and callback of the login, and the claims
/// of the ID token it is exchanged for.
pub struct Grant {
  /// the PKCE challenge the login sent, which the code's verifier must match
  pub challenge: String,
  /// the callback the login named, which the exchange must name again
  pub redirect_uri: String,
  /// the claims of the ID token, besides `iss`, `aud`, `iat` and `exp`
  pub claims: Value,
}

/// The running provider.
pub struct Issuer {
  /// its issuer URL, without a trailing slash
  pub url: String,
  shared: Arc<Shared>,
  server: tokio::task::JoinHandle<()>,
}

struct Shared {
  url: String,
  key: CoreRsaPrivateSigningKey,
  codes: Mutex<HashMap<String, Grant>>,
  userinfo: Mutex<Value>,
}

impl Drop for Issuer {
  fn drop(&mut self) {
    self.server.abort();
  }
}

impl Issuer {
  /// Starts the provider on a free local port.
  pub async fn start() -> Self {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_RSA_SHA256).unwrap();
    let pem = rsa::RsaPrivateKey::from_pkcs8_der(&pair.serialize_der())
      .unwrap()
      .to_pkcs1_pem(rsa::pkcs8::LineEnding::LF)
      .unwrap();
    let key =
      CoreRsaPrivateSigningKey::from_pem(&pem, Some(JsonWebKeyId::new("k1".into()))).unwrap();
    let shared = Arc::new(Shared {
      url: url.clone(),
      key,
      codes: Mutex::new(HashMap::new()),
      userinfo: Mutex::new(json!({})),
    });
    let router = Router::new()
      .route("/.well-known/openid-configuration", get(discovery))
      .route("/jwks", get(jwks))
      .route("/token", post(token))
      .route("/userinfo", get(userinfo))
      .with_state(shared.clone());
    let server = tokio::spawn(async move {
      axum::serve(listener, router).await.unwrap();
    });
    Self {
      url,
      shared,
      server,
    }
  }

  /// Grants `code`, as the provider's login page would after the user signed in.
  pub fn grant(&self, code: &str, grant: Grant) {
    self
      .shared
      .codes
      .lock()
      .unwrap()
      .insert(code.to_string(), grant);
  }

  /// Sets what userinfo returns.
  pub fn set_userinfo(&self, claims: Value) {
    *self.shared.userinfo.lock().unwrap() = claims;
  }

  /// `claims` signed as a JWT with the provider's key.
  pub fn sign(&self, claims: &Value) -> String {
    self.shared.sign(claims)
  }
}

impl Shared {
  fn sign(&self, claims: &Value) -> String {
    let header =
      URL_SAFE_NO_PAD.encode(json!({ "alg": "RS256", "kid": "k1", "typ": "JWT" }).to_string());
    let body = URL_SAFE_NO_PAD.encode(claims.to_string());
    let message = format!("{header}.{body}");
    let signature = self
      .key
      .sign(
        &CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
        message.as_bytes(),
      )
      .unwrap();
    format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature))
  }
}

async fn discovery(State(shared): State<Arc<Shared>>) -> Json<Value> {
  let url = &shared.url;
  Json(json!({
    "issuer": url,
    "authorization_endpoint": format!("{url}/authorize"),
    "token_endpoint": format!("{url}/token"),
    "userinfo_endpoint": format!("{url}/userinfo"),
    "jwks_uri": format!("{url}/jwks"),
    "end_session_endpoint": format!("{url}/logout"),
    "response_types_supported": ["code"],
    "subject_types_supported": ["public"],
    "id_token_signing_alg_values_supported": ["RS256"],
    "token_endpoint_auth_methods_supported": ["client_secret_basic"],
  }))
}

async fn jwks(State(shared): State<Arc<Shared>>) -> Json<Value> {
  Json(json!({ "keys": [shared.key.as_verification_key()] }))
}

async fn token(
  State(shared): State<Arc<Shared>>,
  headers: HeaderMap,
  Form(form): Form<HashMap<String, String>>,
) -> Response {
  let refuse =
    |error: &str| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
  let basic = format!(
    "Basic {}",
    STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"))
  );
  if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some(basic.as_str()) {
    return refuse("invalid_client");
  }
  let Some(grant) = form
    .get("code")
    .and_then(|code| shared.codes.lock().unwrap().remove(code))
  else {
    return refuse("invalid_grant");
  };
  let verifier = form.get("code_verifier").cloned().unwrap_or_default();
  if URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != grant.challenge {
    return refuse("invalid_grant");
  }
  if form.get("redirect_uri") != Some(&grant.redirect_uri) {
    return refuse("invalid_grant");
  }
  let now = chrono::Utc::now().timestamp();
  let mut claims = json!({ "iss": shared.url, "aud": CLIENT_ID, "iat": now, "exp": now + 300 });
  for (name, value) in grant.claims.as_object().unwrap() {
    claims[name] = value.clone();
  }
  Json(json!({
    "access_token": "access",
    "token_type": "Bearer",
    "expires_in": 300,
    "id_token": shared.sign(&claims),
  }))
  .into_response()
}

async fn userinfo(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
  if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer access") {
    return StatusCode::UNAUTHORIZED.into_response();
  }
  Json(shared.userinfo.lock().unwrap().clone()).into_response()
}
