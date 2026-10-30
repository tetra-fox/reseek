//! Signing in with an OpenID Connect provider: the Authorization Code flow with PKCE. The
//! `state`, the `nonce` and the PKCE verifier go to the browser only sealed in a cookie, so the
//! callback can check each against what it sent and the browser cannot change them.

use std::time::Duration;

use api_types::auth::Role;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use openidconnect::core::{CoreAuthenticationFlow, CoreGenderClaim, CoreJwsSigningAlgorithm};
use openidconnect::{
  AuthorizationCode, ClaimsVerificationError, CsrfToken, IdTokenClaims, Nonce, OAuth2TokenResponse,
  PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, SignatureVerificationError,
  TokenResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::debug;

use super::{ExtraClaims, LEEWAY, Metadata, OidcClient, OidcIdToken, Provider, error_chain};
use crate::auth::roles::{Mapping, claim_strings};
use crate::auth::secret::Secret;
use crate::crypto;

/// How long a login may take between leaving for the provider and coming back.
pub const LOGIN_TIME: Duration = Duration::from_secs(600);

/// What the login cookie holds, sealed.
#[derive(Serialize, Deserialize)]
pub struct LoginState {
  state: String,
  nonce: String,
  verifier: String,
  /// the path on this site to return to
  pub redirect: String,
  /// when the login started, in seconds since the epoch
  created: i64,
}

/// The login cookie's name for the provider `name`.
pub fn cookie_name(name: &str) -> String {
  format!("oidc_{name}")
}

/// Whether `redirect` is a path on this site: it starts with one `/` and has no `\`, which
/// browsers read as `/`.
pub fn is_local_path(redirect: &str) -> bool {
  redirect.starts_with('/')
    && !redirect.starts_with("//")
    && !redirect.contains('\\')
    && !redirect.chars().any(char::is_control)
}

/// Where to send the browser to sign in at the provider, and the sealed cookie that remembers
/// what was sent, for a login that returns to `redirect` and comes back to `callback`.
pub fn begin(
  provider: &Provider,
  metadata: Metadata,
  callback: RedirectUrl,
  redirect: String,
  secret: &Secret,
) -> (String, String) {
  let random = || URL_SAFE_NO_PAD.encode(crypto::random_bytes(32));
  let (state, nonce) = (random(), random());
  let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
  let client = provider.client(metadata, callback);
  let sent_state = state.clone();
  let sent_nonce = nonce.clone();
  let mut request = client.authorize_url(
    CoreAuthenticationFlow::AuthorizationCode,
    move || CsrfToken::new(sent_state),
    move || Nonce::new(sent_nonce),
  );
  for scope in &provider.options.scopes {
    // the client always asks for openid itself
    if scope != "openid" {
      request = request.add_scope(Scope::new(scope.clone()));
    }
  }
  let (url, _, _) = request.set_pkce_challenge(challenge).url();
  let sealed = secret.seal(
    &serde_json::to_vec(&LoginState {
      state,
      nonce,
      verifier: verifier.secret().clone(),
      redirect,
      created: Utc::now().timestamp(),
    })
    .expect("the login state is plain data"),
  );
  (url.to_string(), sealed)
}

/// The login a sealed cookie remembers, when it was sealed here, is no older than
/// [`LOGIN_TIME`] and sent the `state` the provider returned.
pub fn opened(secret: &Secret, sealed: &str, state: &str) -> Option<LoginState> {
  let login: LoginState = serde_json::from_slice(&secret.open(sealed)?).ok()?;
  let age = Utc::now().timestamp() - login.created;
  let fresh = (0..=LOGIN_TIME.as_secs() as i64).contains(&age);
  let same: bool = subtle::ConstantTimeEq::ct_eq(login.state.as_bytes(), state.as_bytes()).into();
  (fresh && same).then_some(login)
}

/// What the provider said about who signed in.
pub struct Claims {
  /// the ID token's claims, with those from the userinfo endpoint added when the ID token
  /// lacked the groups
  pub claims: Value,
  /// the subject, the provider's stable id for the user
  pub subject: String,
  /// the provider's session id, for its back-channel logout
  pub sid: Option<String>,
  /// the ID token, for logging out at the provider
  pub id_token: String,
}

/// Exchanges `code` for tokens and checks the ID token: its issuer, audience, times, nonce and
/// signature. The provider's keys are read again once when the token names a key not seen yet.
pub async fn exchange(
  provider: &Provider,
  metadata: Metadata,
  callback: RedirectUrl,
  code: String,
  login: &LoginState,
) -> Result<Claims, String> {
  let client = provider.client(metadata.clone(), callback.clone());
  let response = client
    .exchange_code(AuthorizationCode::new(code))
    .map_err(|e| error_chain(&e))?
    .set_pkce_verifier(PkceCodeVerifier::new(login.verifier.clone()))
    .request_async(provider.http())
    .await
    .map_err(|e| format!("the token endpoint refused the code: {}", error_chain(&e)))?;
  let id_token = response
    .id_token()
    .ok_or_else(|| "the provider sent no ID token".to_string())?
    .clone();
  let nonce = Nonce::new(login.nonce.clone());
  let verified = match verify(&client, &id_token, &nonce) {
    Err(ClaimsVerificationError::SignatureVerification(
      SignatureVerificationError::NoMatchingKey,
    )) => match provider.refresh_keys().await {
      Some(fresh) => {
        let client = provider.client(fresh, callback);
        verify(&client, &id_token, &nonce)
      }
      None => Err(ClaimsVerificationError::SignatureVerification(
        SignatureVerificationError::NoMatchingKey,
      )),
    },
    other => other,
  };
  let verified = verified.map_err(|e| format!("the ID token is not valid: {e}"))?;
  let subject = verified.subject().clone();
  // the verified claims as one object, standard and extra alike, so any claim can be named
  let mut claims = serde_json::to_value(&verified).map_err(|e| e.to_string())?;
  let claim = &provider.options.role_mapping.claim;
  if claim_strings(&claims, claim).is_empty() && metadata.userinfo_endpoint().is_some() {
    let answer = match client.user_info(response.access_token().clone(), Some(subject.clone())) {
      Ok(request) => request
        .request_async::<ExtraClaims, _, CoreGenderClaim>(provider.http())
        .await
        .map_err(|e| error_chain(&e)),
      Err(e) => Err(error_chain(&e)),
    };
    match answer.and_then(|info| serde_json::to_value(&info).map_err(|e| e.to_string())) {
      Ok(Value::Object(more)) => {
        if let Value::Object(claims) = &mut claims {
          for (key, value) in more {
            claims.entry(key).or_insert(value);
          }
        }
      }
      Ok(_) => debug!(
        provider = provider.name,
        "userinfo was not an object; ignored"
      ),
      Err(e) => debug!(provider = provider.name, error = e, "userinfo not read"),
    }
  }
  Ok(Claims {
    sid: claims
      .get("sid")
      .and_then(Value::as_str)
      .map(str::to_string),
    claims,
    subject: subject.to_string(),
    id_token: id_token.to_string(),
  })
}

/// The claims of `id_token` when it is valid for `client`, signed with RS256 or ES256.
fn verify(
  client: &OidcClient,
  id_token: &OidcIdToken,
  nonce: &Nonce,
) -> Result<IdTokenClaims<ExtraClaims, CoreGenderClaim>, ClaimsVerificationError> {
  let verifier = client
    .id_token_verifier()
    .set_allowed_algs([
      CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
      CoreJwsSigningAlgorithm::EcdsaP256Sha256,
    ])
    .set_time_fn(|| Utc::now() - LEEWAY)
    .set_issue_time_verifier_fn(|issued| {
      if issued <= Utc::now() + LEEWAY {
        Ok(())
      } else {
        Err(format!("issued in the future, at {issued}"))
      }
    });
  id_token.claims(&verifier, nonce).cloned()
}

/// Who signed in, once the provider's rules let them in.
pub struct Admitted {
  /// the role their groups give
  pub role: Role,
  /// the name to show
  pub name: String,
  /// their email address
  pub email: Option<String>,
  /// their groups
  pub groups: Vec<String>,
}

/// Applies the provider's allow-lists and role mapping to `claims`; why not when the user is
/// refused.
pub fn admit(provider: &Provider, claims: &Claims) -> Result<Admitted, String> {
  let options = &provider.options;
  let claim = |name: &str| claims.claims.get(name).and_then(Value::as_str);
  let verified_email = claim("email").filter(|_| {
    claims
      .claims
      .get("email_verified")
      .and_then(Value::as_bool)
      .unwrap_or(false)
  });
  if !options.allowed_subjects.is_empty() && !options.allowed_subjects.contains(&claims.subject) {
    return Err("the subject is not in allowed_subjects".to_string());
  }
  if !options.allowed_emails.is_empty()
    && !verified_email.is_some_and(|email| options.allowed_emails.iter().any(|e| e == email))
  {
    return Err("no verified email in allowed_emails".to_string());
  }
  let groups = claim_strings(&claims.claims, &options.role_mapping.claim);
  let mapping = &options.role_mapping;
  let role = Mapping {
    administrator: &mapping.administrator,
    readwrite: &mapping.readwrite,
    readonly: &mapping.readonly,
    default: mapping.default,
  }
  .role(&groups)
  .ok_or_else(|| "in no group that grants a role".to_string())?;
  let name = claim(&options.username_claim)
    .or_else(|| claim("email"))
    .unwrap_or(&claims.subject)
    .to_string();
  Ok(Admitted {
    role,
    name,
    email: claim("email").map(str::to_string),
    groups,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn only_paths_on_this_site_are_returned_to() {
    assert!(is_local_path("/"));
    assert!(is_local_path("/#/transfers?x=1"));
    assert!(!is_local_path("//evil.example/"));
    assert!(!is_local_path("/\\evil.example"));
    assert!(!is_local_path("https://evil.example/"));
    assert!(!is_local_path("transfers"));
    assert!(!is_local_path("/a\nb"));
  }
}
