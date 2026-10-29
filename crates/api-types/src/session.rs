//! Signing in and out: the login providers, the current principal, local password login,
//! OpenID Connect redirects and the list of browser sessions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::auth::{Principal, Role, Via};

/// The ways to sign in that the login page offers, as `GET /session/providers` returns them to
/// anyone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Providers {
  /// password login with a local account
  pub local: LocalProvider,
  /// the OpenID Connect providers, one login button each, in order of their names
  pub oidc: Vec<OidcProvider>,
  /// identity forwarded by a trusted reverse proxy
  pub proxy: ProxyProvider,
  /// the caller's address is in a trusted network, so the login form can be skipped
  pub trusted_network: bool,
}

/// Password login with a local account.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalProvider {
  /// password login is on and allowed from the caller's address
  pub enabled: bool,
}

/// One OpenID Connect provider's login button.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OidcProvider {
  /// the provider's configured name
  #[schema(example = "authentik")]
  pub name: String,
  /// the button's label
  #[schema(example = "Home SSO")]
  pub display_name: String,
  /// where the button leads to start the login
  #[schema(example = "/api/v0/auth/oidc/authentik/login")]
  pub login_url: String,
  /// the login page goes straight to this provider without showing itself
  pub auto_login: bool,
}

/// Identity forwarded by a trusted reverse proxy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProxyProvider {
  /// the proxy's identity headers are honored
  pub enabled: bool,
}

/// Who the caller is, as `GET /session` returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrentPrincipal {
  /// the principal the request acts as
  #[serde(flatten)]
  pub principal: Principal,
  /// the browser session the request came with, null for an API token, a config API key or a
  /// trusted network
  pub session: Option<CurrentSession>,
  /// the provider the identity came from: `local`, `proxy` or an OpenID Connect provider's
  /// name; null when there is no session
  #[schema(example = "authentik")]
  pub provider: Option<String>,
}

/// The browser session a request came with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSession {
  /// the session's id
  pub id: String,
  /// when the login happened
  pub created_at: DateTime<Utc>,
  /// when the session ends regardless of use
  pub expires_at: DateTime<Utc>,
}

/// A local account's username and password, the body of `POST /session`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
  /// the account's username
  #[schema(example = "admin")]
  pub username: String,
  /// the account's password
  #[schema(format = Password)]
  pub password: String,
}

/// What a logout did, as `POST /session/logout` returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogoutResponse {
  /// the identity provider's logout page for the browser to go to, null when the session was
  /// not from an OpenID Connect provider that offers one
  pub redirect: Option<String>,
}

/// An active browser session, an entry of `GET /sessions`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Session {
  /// the session's id
  pub id: String,
  /// the stable id of who signed in, such as `local:admin`
  #[schema(example = "local:admin")]
  pub subject: String,
  /// the name to show for who signed in
  pub name: String,
  /// how the session was established: a local login, an OpenID Connect login or a reverse
  /// proxy
  pub via: Via,
  /// the role, fixed at login
  pub role: Role,
  /// when the login happened
  pub created_at: DateTime<Utc>,
  /// when the session was last used, to within five minutes
  pub last_seen_at: DateTime<Utc>,
  /// the client address it was last used from, null when unknown
  pub ip: Option<String>,
  /// the browser's user agent, null when it sent none
  pub user_agent: Option<String>,
  /// this is the session the request came with
  pub current: bool,
}

/// The query of `GET /auth/oidc/{name}/login`, which starts an OpenID Connect login.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct OidcLoginQuery {
  /// the path on this site to return to after the login, `/` when left out; anything that is
  /// not a path on this site is refused
  #[param(example = "/transfers")]
  pub redirect: Option<String>,
}

/// The query the identity provider sends the browser back with to
/// `GET /auth/oidc/{name}/callback`. The names are the OAuth 2.0 ones.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "snake_case")]
#[into_params(parameter_in = Query)]
pub struct OidcCallbackQuery {
  /// the authorization code to exchange, null when the provider sent an error
  pub code: Option<String>,
  /// the value sent with the login, matched against the login cookie
  pub state: Option<String>,
  /// the provider's error code when it refused the login
  pub error: Option<String>,
  /// the provider's explanation of `error`, shown on the login page as text
  pub error_description: Option<String>,
}

/// The form an identity provider posts to `POST /auth/oidc/{name}/backchannel` to end
/// sessions it started. The name is the OpenID Connect one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct BackchannelLogout {
  /// the signed logout token naming the provider session or the subject to sign out
  pub logout_token: String,
}

/// The outcome of reading a configured OpenID Connect provider's discovery document, as
/// `POST /auth/oidc/{name}/test` returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OidcTestResult {
  /// the discovery document was read and names the configured issuer
  pub ok: bool,
  /// the issuer the discovery document names, null when it could not be read
  #[schema(example = "https://auth.example.net/application/o/reseek/")]
  pub issuer: Option<String>,
  /// the endpoints the discovery document lists, null when it could not be read
  pub endpoints: Option<OidcEndpoints>,
  /// why the test failed, null when it passed
  pub error: Option<String>,
}

/// The endpoints an OpenID Connect discovery document lists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OidcEndpoints {
  /// where the browser is sent to sign in
  pub authorization: String,
  /// where the authorization code is exchanged
  pub token: String,
  /// where claims missing from the ID token are fetched, null when the provider has none
  pub userinfo: Option<String>,
  /// where the provider's signing keys are published
  pub jwks: String,
  /// where the browser is sent to sign out of the provider, null when the provider has none
  pub end_session: Option<String>,
}
