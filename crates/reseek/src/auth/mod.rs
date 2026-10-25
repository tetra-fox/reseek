//! Who a request is, and what it may do.
//!
//! [`authenticate`] runs on every API request and settles the request's principal, trying in
//! order: an API token, a configuration API key, a trusted network. A credential that is
//! presented but not valid fails the request at once rather than falling through to a weaker
//! way in. The principal, or its absence, is put on the request as an [`Authentication`]; a
//! handler takes an [`Authorized`] to require a role.

pub mod client;
pub mod keys;
pub mod networks;
pub mod store;

use std::marker::PhantomData;

use api_types::auth::{Principal, Role, Via};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::api::error::ApiError;
use crate::app::AppState;
use crate::http::Peer;
pub use client::Client;

/// The prefix of an API token.
pub const TOKEN_PREFIX: &str = "slsk_";

/// The principal of a request, `None` when it presented nothing that identifies it.
#[derive(Clone, Debug)]
pub struct Authentication(pub Option<Principal>);

/// Settles the principal of a request and puts it, with the [`Client`], on the request.
pub async fn authenticate(
  State(app): State<AppState>,
  mut request: Request,
  next: Next,
) -> Response {
  let Some(peer) = request.extensions().get::<Peer>().cloned() else {
    return ApiError::internal("a request arrived without its connection").into_response();
  };
  let current = app.config.current();
  let options = &current.values;
  let proxies = networks::ranges(&options.web.trusted_proxies);
  let client = client::resolve(&peer, request.uri(), request.headers(), &proxies);
  let principal = match principal(&app, options, &client, request.headers()) {
    Ok(principal) => principal,
    Err(e) => return e.into_response(),
  };
  if let Some(principal) = &principal {
    tracing::Span::current().record("principal", principal.subject.as_str());
  }
  request.extensions_mut().insert(client);
  request.extensions_mut().insert(Authentication(principal));
  next.run(request).await
}

/// The chain itself: the first way in that applies decides.
fn principal(
  app: &AppState,
  options: &crate::config::Options,
  client: &Client,
  headers: &HeaderMap,
) -> Result<Option<Principal>, ApiError> {
  if let Some(presented) = presented_credential(headers) {
    if presented.starts_with(TOKEN_PREFIX) {
      tracing::debug!(ip = %client.ip, length = presented.len(), "unknown API token");
      return Err(ApiError::unauthorized("unknown API token").logged());
    }
    return match keys::check(
      presented,
      client.ip,
      app.primary_key.as_deref(),
      &options.web.authentication.api_keys,
    ) {
      Ok(principal) => Ok(Some(principal)),
      Err(refusal) => {
        // the key itself is never logged, only how long it is
        tracing::debug!(ip = %client.ip, length = presented.len(), reason = %refusal, "API key refused");
        Err(ApiError::unauthorized(refusal.to_string()).logged())
      }
    };
  }
  Ok(networks::trusted(client, options))
}

/// The credential in `Authorization: Bearer` or `X-API-Key`, in that order.
fn presented_credential(headers: &HeaderMap) -> Option<&str> {
  let bearer = headers
    .get(header::AUTHORIZATION)
    .and_then(|h| h.to_str().ok())
    .and_then(|value| {
      let (scheme, credential) = value.split_once(' ')?;
      scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(credential.trim())
    });
  bearer
    .or_else(|| headers.get("x-api-key").and_then(|h| h.to_str().ok()))
    .map(str::trim)
    .filter(|credential| !credential.is_empty())
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
      Some(Authentication(None)) => return Err(ApiError::unauthorized("authentication required")),
      None => {
        return Err(ApiError::internal(
          "a route that needs a role is not authenticated",
        ));
      }
    };
    if principal.role < R::ROLE {
      return Err(ApiError::forbidden(format!("needs the {:?} role", R::ROLE)));
    }
    Ok(Self {
      principal,
      _role: PhantomData,
    })
  }
}

#[cfg(test)]
mod tests {
  use crate::testing::{app, request};

  const CONFIG: &str = "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n\
                        [web.authentication.api_keys.viewer]\nkey = \"viewer-key-0123456789\"\n";

  fn with_header(name: &'static str, value: &str) -> axum::http::Request<axum::body::Body> {
    let mut r = request("GET", "/api/v0/options", None);
    r.headers_mut().insert(name, value.parse().unwrap());
    r
  }

  #[tokio::test]
  async fn a_bad_credential_fails_even_from_a_trusted_network() {
    let app = app(CONFIG, &[]);
    assert_eq!(
      app
        .send(request("GET", "/api/v0/options", None))
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
    assert_eq!(token.json()["error"], "unknown API token");
  }

  #[tokio::test]
  async fn roles_and_the_rebinding_rule_decide_the_rest() {
    let app = app(CONFIG, &[]);
    let viewer = app
      .send_from(
        "192.0.2.1:1",
        with_header("authorization", "Bearer viewer-key-0123456789"),
      )
      .await;
    assert_eq!(viewer.status, 403, "a read-only key is no administrator");
    let stranger = app
      .send_from("192.0.2.1:1", request("GET", "/api/v0/options", None))
      .await;
    assert_eq!(stranger.status, 401);
    let mut rebound = request("GET", "/api/v0/options", None);
    rebound
      .headers_mut()
      .insert("host", "attacker.example".parse().unwrap());
    assert_eq!(app.send(rebound).await.status, 401);
  }
}
