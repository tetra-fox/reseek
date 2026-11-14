//! Who a request is, and what it may do.
//!
//! [`authenticate`] runs on every API request and settles the request's principal, trying in
//! order: an API token, a configuration API key, the session cookie, a trusted reverse proxy's
//! forwarded identity, a trusted network. A token or key that is presented but not valid fails
//! the request at once rather than falling through to a weaker way in; a session cookie that is
//! not valid is cleared and the next ways are tried. A request that changes something on the
//! strength of the session cookie alone must come from a page of this site, and no request let
//! in by a trusted network or a proxy's forwarded identity may come from a page of another site.
//! The principal, or its absence, is put on the request as an [`Authentication`]; a handler
//! takes an [`Authorized`] to require a role.

mod chain;
pub mod client;
pub mod cookies;
pub mod csrf;
pub mod keys;
pub mod limits;
pub mod local;
pub mod networks;
pub mod oidc;
pub mod proxy;
pub mod roles;
pub mod secret;
pub mod sessions;
pub mod store;
pub mod tokens;

use std::marker::PhantomData;

use api_types::auth::{Principal, Role, Via};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::header;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::debug;

use crate::api::error::ApiError;
use crate::app::AppState;
use crate::db::{Database, DbError, Location};
use crate::http::Peer;
pub use chain::{
  Check, Identified, Presented, end_sessions, end_sessions_without_role, identify, idle,
  page_session, session_cookie, start_session,
};
pub use client::Client;
use limits::Limits;
use local::Accounts;
use secret::Secret;
use sessions::StoredSession;

/// The prefix of an API token.
pub const TOKEN_PREFIX: &str = "slsk_";

/// What the daemon keeps to tell who a request is: the authentication database, the key its
/// cookies are made with, the failure limits and the local accounts.
pub struct Auth {
  /// `auth.db`
  pub db: Database,
  /// the key session cookies and sealed state are made with
  pub secret: Secret,
  /// failed attempts by address
  pub limits: Limits,
  /// the local accounts with their password hashes
  pub accounts: Accounts,
  /// the OpenID Connect providers in use
  pub oidc: oidc::Providers,
}

impl Auth {
  /// Opens `auth.db` in `location` and loads the cookie key from it.
  pub async fn open(location: &Location) -> Result<Self, DbError> {
    let db = Database::open(&store::SCHEMA, location).await?;
    let secret = Secret::load(&db).await?;
    Ok(Self {
      db,
      secret,
      limits: Limits::default(),
      accounts: Accounts::default(),
      oidc: oidc::Providers::default(),
    })
  }
}

/// The principal of a request, `None` when it presented nothing that identifies it.
#[derive(Clone, Debug)]
pub struct Authentication(pub Option<Principal>);

/// The browser session a request came with, when its cookie was valid.
#[derive(Clone, Debug)]
pub struct RequestSession(pub StoredSession);

/// Settles the principal of a request and puts it, with the [`Client`], on the request.
pub async fn authenticate(
  State(app): State<AppState>,
  mut request: Request,
  next: Next,
) -> Response {
  let Some(peer) = request.extensions().get::<Peer>().copied() else {
    return ApiError::internal("a request arrived without its connection").into_response();
  };
  let current = app.config.current();
  let presented = Presented::read(&peer, request.uri(), request.headers(), &current.values);
  let identified = match identify(&app, &presented, Check::Request).await {
    Ok(identified) => identified,
    Err(e) => return e.into_response(),
  };
  let headers = request.headers();
  let client = &presented.client;
  // a browser on a trusted network, or behind a proxy that signs it in from its own cookies, is
  // let in on any request a page makes it send. Another site's page may also read the answer
  // when `web.cors.allowed_origins` has `*`, so reads are refused as well as changes
  let ambient = identified
    .principal
    .as_ref()
    .is_some_and(|p| matches!(p.via, Via::TrustedNetwork { .. } | Via::Proxy));
  // a browser shows another site the answer to a cookie request only for a listed CORS origin
  let cookie_change = identified.by_cookie && csrf::changes_state(request.method());
  let cross_site = (ambient && csrf::foreign_origin(headers, client, &current.values))
    || (cookie_change && !csrf::same_origin(headers, client, &current.values));
  if cross_site {
    let via = identified.principal.as_ref().map(|p| &p.via);
    debug!(ip = %client.ip, ?via, "cross-site request refused");
    return ApiError::forbidden("Cross-site request refused")
      .logged()
      .into_response();
  }
  if let Some(principal) = &identified.principal {
    tracing::Span::current().record("principal", principal.subject.as_str());
  }
  request.extensions_mut().insert(presented.client.clone());
  request
    .extensions_mut()
    .insert(Authentication(identified.principal));
  if let Some(session) = identified.session {
    request.extensions_mut().insert(RequestSession(session));
  }
  request.extensions_mut().insert(presented);
  let mut response = next.run(request).await;
  for cookie in identified.cookies {
    response.headers_mut().append(header::SET_COOKIE, cookie);
  }
  response
}

/// Whether `principal` came from someone at the UI rather than from a stored credential: a
/// login session or a trusted network. Changing the process or its configuration files needs
/// this.
pub fn is_interactive(principal: &Principal) -> bool {
  matches!(
    principal.via,
    Via::Local | Via::Oidc { .. } | Via::Proxy | Via::TrustedNetwork { .. }
  )
}

/// Refuses a principal that is a stored credential rather than someone at the UI.
pub fn require_interactive(principal: &Principal) -> Result<(), ApiError> {
  if is_interactive(principal) {
    Ok(())
  } else {
    Err(ApiError::forbidden(
      "Requires a login session. An API key or token cannot perform this action.",
    ))
  }
}

/// The least role a handler takes, as a type.
pub trait MinimumRole {
  /// the role
  const ROLE: Role;
}

/// Any principal.
pub struct AnyRole;
/// A principal that may read and write.
pub struct ReadWrite;
/// An administrator.
pub struct Administrator;

impl MinimumRole for AnyRole {
  const ROLE: Role = Role::ReadOnly;
}

impl MinimumRole for ReadWrite {
  const ROLE: Role = Role::ReadWrite;
}

impl MinimumRole for Administrator {
  const ROLE: Role = Role::Administrator;
}

/// The principal of a request that has at least the role `R`: a request without one is
/// answered 401, one whose role is too low 403.
pub struct Authorized<R> {
  /// the principal
  pub principal: Principal,
  _role: PhantomData<R>,
}

impl<S: Send + Sync, R: MinimumRole> FromRequestParts<S> for Authorized<R> {
  type Rejection = ApiError;

  async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, ApiError> {
    let principal = match parts.extensions.get::<Authentication>() {
      Some(Authentication(Some(principal))) => principal.clone(),
      Some(Authentication(None)) => return Err(ApiError::unauthorized("Authentication required")),
      None => {
        return Err(ApiError::internal(
          "a route that needs a role is not authenticated",
        ));
      }
    };
    if principal.role < R::ROLE {
      return Err(ApiError::forbidden(format!(
        "Requires the {:?} role",
        R::ROLE
      )));
    }
    Ok(Self {
      principal,
      _role: PhantomData,
    })
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use crate::testing::{OUTSIDE, app, request};

  /// Loopback is a trusted network, and the key `viewer-key-0123456789` is a viewer's.
  pub(crate) const CONFIG: &str = "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n\
                        [web.authentication.api_keys.viewer]\nkey = \"viewer-key-0123456789\"\n";

  /// A request for the options view, which takes an administrator, with the header `name` set
  /// to `value`.
  pub(crate) fn with_header(
    name: &'static str,
    value: &str,
  ) -> axum::http::Request<axum::body::Body> {
    let mut r = request("GET", "/api/v0/options/view", None);
    r.headers_mut().insert(name, value.parse().unwrap());
    r
  }

  #[tokio::test]
  async fn a_bad_credential_fails_even_from_a_trusted_network() {
    let app = app(CONFIG, &[]).await;
    assert_eq!(
      app
        .send(request("GET", "/api/v0/options/view", None))
        .await
        .status,
      200
    );
    let wrong = app
      .send(with_header("x-api-key", "not-a-configured-key"))
      .await;
    assert_eq!(wrong.status, 401);
    assert_eq!(wrong.headers["www-authenticate"], "Bearer");
    let token = app
      .send(with_header("authorization", "Bearer slsk_0123456789"))
      .await;
    assert_eq!(token.status, 401);
    assert_eq!(token.json()["error"], "Unknown API token");
  }

  #[tokio::test]
  async fn a_trusted_network_request_may_not_come_from_another_site() {
    let app = app(
      &format!(
        "remote_configuration = true\n[web.cors]\nallowed_origins = [\"*\"]\n{CONFIG}\
         [web.authentication.api_keys.robot]\nkey = \"robot-key-0123456789\"\n\
         role = \"administrator\"\n"
      ),
      &[],
    )
    .await;
    let patch = |origin: Option<&str>| {
      let mut r = request("PATCH", "/api/v0/options", Some(serde_json::json!({})));
      if let Some(origin) = origin {
        r.headers_mut().insert("origin", origin.parse().unwrap());
      }
      r
    };
    let foreign = app.send(patch(Some("http://evil.example"))).await;
    assert_eq!(foreign.status, 403);
    assert_eq!(foreign.json()["error"], "Cross-site request refused");
    assert_eq!(app.send(patch(Some("null"))).await.status, 403);
    assert_eq!(app.send(patch(Some("http://localhost"))).await.status, 204);
    assert_eq!(
      app.send(patch(None)).await.status,
      204,
      "scripts send no Origin"
    );
    let read = app.send(with_header("origin", "http://evil.example")).await;
    assert_eq!(
      read.status, 403,
      "CORS lets another site read the answer, so reads are refused too"
    );
    let own = with_header("origin", "http://localhost");
    assert_eq!(app.send(own).await.status, 200);
    let keyed = crate::testing::with(
      request("GET", "/api/v0/session", None),
      "x-api-key",
      "viewer-key-0123456789",
    );
    let keyed = crate::testing::with(keyed, "origin", "http://evil.example");
    assert_eq!(
      app.send(keyed).await.status,
      200,
      "a key is no ambient credential"
    );
    let keyed = crate::testing::with(
      patch(Some("http://evil.example")),
      "x-api-key",
      "robot-key-0123456789",
    );
    assert_eq!(
      app.send_from(OUTSIDE, keyed).await.status,
      204,
      "nor for a change"
    );
  }

  #[tokio::test]
  async fn a_proxy_identity_change_may_not_come_from_another_site() {
    let app = app(
      "remote_configuration = true\n[web]\ntrusted_proxies = [\"127.0.0.1/32\"]\n\
       [web.authentication.proxy]\nenabled = true\n\
       [web.authentication.proxy.role_mapping]\nadministrator = [\"admins\"]\n",
      &[],
    )
    .await;
    let mut patch = request("PATCH", "/api/v0/options", Some(serde_json::json!({})));
    let headers = patch.headers_mut();
    headers.insert("x-authentik-username", "alice".parse().unwrap());
    headers.insert("x-authentik-groups", "admins".parse().unwrap());
    headers.insert("origin", "http://evil.example".parse().unwrap());
    let foreign = app.send(patch).await;
    assert_eq!(foreign.status, 403);
    assert_eq!(foreign.json()["error"], "Cross-site request refused");
  }

  #[tokio::test]
  async fn roles_and_the_rebinding_rule_decide_the_rest() {
    let app = app(CONFIG, &[]).await;
    let viewer = app
      .send_from(
        OUTSIDE,
        with_header("authorization", "Bearer viewer-key-0123456789"),
      )
      .await;
    assert_eq!(viewer.status, 403, "a read-only key is no administrator");
    let stranger = app
      .send_from(OUTSIDE, request("GET", "/api/v0/options/view", None))
      .await;
    assert_eq!(stranger.status, 401);
    let mut rebound = request("GET", "/api/v0/options/view", None);
    rebound
      .headers_mut()
      .insert("host", "attacker.example".parse().unwrap());
    assert_eq!(app.send(rebound).await.status, 401);
  }
}
